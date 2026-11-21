import Foundation
import LetsDecideFFI

/// Why a request failed, safe to log: names, ids, counts, MIME types. Never
/// keys or content. Fields the core doesn't set are `nil`.
public struct ErrorDetails: Equatable, Sendable {
    public let reason: String
    public var provider: String?
    public let question: String?
    public let id: String?
    public let limit: Int?
    public let actual: Int?
    public let mimeType: String?
    public let field: String?

    public init(
        reason: String, provider: String? = nil, question: String? = nil, id: String? = nil,
        limit: Int? = nil, actual: Int? = nil, mimeType: String? = nil, field: String? = nil
    ) {
        self.reason = reason
        self.provider = provider
        self.question = question
        self.id = id
        self.limit = limit
        self.actual = actual
        self.mimeType = mimeType
        self.field = field
    }

    init?(_ d: [String: Any]?) {
        guard let d, let reason = d["reason"] as? String else { return nil }
        self.init(
            reason: reason, provider: d["provider"] as? String, question: d["question"] as? String,
            id: d["id"] as? String, limit: d["limit"] as? Int, actual: d["actual"] as? Int,
            mimeType: d["mime_type"] as? String, field: d["field"] as? String)
    }
}

/// A typed error from any provider. Never carries keys or request bodies.
public struct LetsDecideError: Error, Equatable {
    public let code: String
    public let message: String
    public let status: Int?
    public let retryAfterMs: Int?
    public let retryable: Bool
    public var details: ErrorDetails?

    public init(
        code: String, message: String, status: Int? = nil, retryAfterMs: Int? = nil,
        retryable: Bool = false, details: ErrorDetails? = nil
    ) {
        self.code = code
        self.message = message
        self.status = status
        self.retryAfterMs = retryAfterMs
        self.retryable = retryable
        self.details = details
    }

    init(_ e: [String: Any]) {
        self.init(
            code: e["code"] as? String ?? "BadResponse", message: e["message"] as? String ?? "",
            status: e["status"] as? Int, retryAfterMs: e["retry_after_ms"] as? Int,
            retryable: e["retryable"] as? Bool ?? false,
            details: ErrorDetails(e["details"] as? [String: Any]))
    }
}

/// The request the core built; a `Transport` sends it.
public struct Http {
    public let method: String
    public let url: URL
    public let headers: [(String, String)]
    public let body: Data
}

/// The provider's reply. Header names are compared case-insensitively.
public struct HttpReply {
    public let status: Int
    public let headers: [(String, String)]
    public let body: Data

    public init(status: Int, headers: [(String, String)], body: Data) {
        self.status = status
        self.headers = headers
        self.body = body
    }
}

/// Sends one request. Swap it for proxies, pinning or tests.
public protocol Transport: Sendable {
    func send(_ http: Http) async throws -> HttpReply
}

/// `URLSession`, 60 s timeout (decisions on media can be slow).
public struct URLSessionTransport: Transport {
    let session: URLSession

    public init(session: URLSession = .shared) {
        self.session = session
    }

    public func send(_ http: Http) async throws -> HttpReply {
        var req = URLRequest(url: http.url, timeoutInterval: 60)
        req.httpMethod = http.method
        req.httpBody = http.body
        for (k, v) in http.headers { req.addValue(v, forHTTPHeaderField: k) }
        let (data, res) = try await session.data(for: req)
        let r = res as! HTTPURLResponse
        let headers = r.allHeaderFields.map { ("\($0.key)", "\($0.value)") }
        return HttpReply(status: r.statusCode, headers: headers, body: data)
    }
}

/// One provider's settings. A provider only ever sees its own entry, so a key
/// can never reach another provider.
public struct ProviderSettings: Sendable {
    public let apiKey: String?
    /// OAuth token provider (Clef), called on every request so the host can
    /// refresh it. Wins over `apiKey`.
    public let accessToken: (@Sendable () async throws -> String)?
    public let baseUrl: String?
    public let accountId: String?
    /// Headers for this provider only; win over the client's shared headers
    /// with the same name.
    public let headers: [(String, String)]
    /// Provider-specific values, e.g. for a dialect's `{options.<name>}` path
    /// placeholders.
    public let options: [String: String]

    public init(
        apiKey: String? = nil,
        accessToken: (@Sendable () async throws -> String)? = nil,
        baseUrl: String? = nil,
        accountId: String? = nil,
        headers: [(String, String)] = [],
        options: [String: String] = [:]
    ) {
        self.apiKey = apiKey
        self.accessToken = accessToken
        self.baseUrl = baseUrl
        self.accountId = accountId
        self.headers = headers
        self.options = options
    }
}

/// Built-in provider names; dialects can't take them without `replace`.
public let BUILTIN: [String] = (try? unwrapAny(LetsDecideFFI.builtins()) as? [String]) ?? []

/// Capabilities of a built-in `"provider/model"`.
public func capabilities(_ model: String) throws -> [String: Any] {
    try unwrap(LetsDecideFFI.capabilities(model: model))
}

/// Validates `request` and, if given, checks it against `capabilities`. Throws `LetsDecideError`.
public func validate(_ request: [String: Any], capabilities: [String: Any]? = nil) throws {
    _ = try unwrapAny(LetsDecideFFI.validate(request: try json(request), capabilities: try capabilities.map(json) ?? "null"))
}

/// Checks a canonical `response` against `request`; returns it with defaults filled.
public func check(_ request: [String: Any], _ response: [String: Any]) throws -> [String: Any] {
    try unwrap(LetsDecideFFI.check(request: try json(request), response: try json(response)))
}

/// The error a built-in adapter would raise for this non-2xx reply. Throw it to raise it.
public func classifyHttp(_ reply: HttpReply) throws -> LetsDecideError {
    LetsDecideError(try unwrap(LetsDecideFFI.classifyHttp(response: try json(replyJson(reply)))))
}

/// A built-in dialect (`jev`, `clef`, `gemini`, `openai`, `systemone`) as JSON, to read or extend.
public func dialect(_ name: String) throws -> [String: Any] {
    try unwrap(LetsDecideFFI.dialect(name: name))
}

/// Validates a dialect and expands `extends`. Throws `LetsDecideError` on a bad one.
public func resolveDialect(_ spec: [String: Any]) throws -> [String: Any] {
    try unwrap(LetsDecideFFI.resolveDialect(dialect: try json(spec)))
}

/// One request shape for every decision API. Requests and responses are JSON
/// dictionaries in the canonical shape (see the README).
public final class Client: Sendable {
    let byProvider: [String: ProviderSettings]
    let headers: [(String, String)]
    let transport: Transport
    /// Resolved once; the core takes the resolved form on every call.
    let dialects: [String: String]

    /// - Parameter byProvider: settings per provider prefix, e.g.
    ///   `["gemini": ProviderSettings(apiKey: key)]`.
    /// - Parameter headers: sent to every provider (tracing, gateways). Never secrets.
    public init(
        byProvider: [String: ProviderSettings] = [:],
        headers: [(String, String)] = [],
        transport: Transport = URLSessionTransport()
    ) {
        self.byProvider = byProvider
        self.headers = headers
        self.transport = transport
        self.dialects = [:]
    }

    /// As `init(byProvider:headers:transport:)`, plus providers described as data.
    /// - Parameter dialects: e.g. `["acme": ["extends": "jev", "base_url": "https://api.acme.ai"]]`
    ///   (see docs/dialects.md). Validated here; throws `LetsDecideError` on a bad one.
    /// - Parameter replace: built-in names a dialect may override (e.g. a patched `gemini`).
    public init(
        byProvider: [String: ProviderSettings] = [:],
        headers: [(String, String)] = [],
        transport: Transport = URLSessionTransport(),
        dialects: [String: [String: Any]],
        replace: Set<String> = []
    ) throws {
        self.byProvider = byProvider
        self.headers = headers
        self.transport = transport
        var resolved: [String: String] = [:]
        for (name, spec) in dialects {
            guard name.range(of: "^[a-z0-9_-]{1,64}$", options: .regularExpression) != nil else {
                throw LetsDecideError(
                    code: "InvalidRequest", message: "provider names must match `[a-z0-9_-]{1,64}`",
                    details: ErrorDetails(reason: "invalid_config", field: "name"))
            }
            if BUILTIN.contains(name) && !replace.contains(name) {
                throw LetsDecideError(
                    code: "InvalidRequest",
                    message: "provider `\(name)` is already registered; list it in `replace` to override it",
                    details: ErrorDetails(reason: "name_taken", provider: name))
            }
            resolved[name] = try tagged(name) { try json(try resolveDialect(spec)) }
        }
        self.dialects = resolved
    }

    /// Capabilities of any `"provider/model"`, built-in or dialect.
    public func capabilities(_ model: String) throws -> [String: Any] {
        guard let d = dialects[prefix(model)] else { return try LetsDecide.capabilities(model) }
        let spec = try JSONSerialization.jsonObject(with: Data(d.utf8)) as? [String: Any]
        return spec?["capabilities"] as? [String: Any] ?? [:]
    }

    /// `model`: `"provider/model"`, e.g. `"clef/clef-flash"`. Throws `LetsDecideError`.
    public func decide(_ model: String, _ request: [String: Any]) async throws -> [String: Any] {
        let req = try json(request)
        let name = prefix(model)
        let config = try json(try await config(name))
        if let d = dialects[name] {
            let rest = String(model.dropFirst(name.count + 1))
            guard model.contains("/"), !rest.isEmpty else {
                throw LetsDecideError(code: "InvalidRequest", message: "model must look like `provider/model`")
            }
            let built = try tagged(name) { try unwrap(LetsDecideFFI.buildDialect(dialect: d, model: rest, request: req, config: config)) }
            let res = try json(replyJson(try await send(built)))
            return try tagged(name) { try unwrap(LetsDecideFFI.parseDialect(dialect: d, request: req, response: res)) }
        }
        let built = try unwrap(LetsDecideFFI.build(model: model, request: req, config: config))
        let res = try json(replyJson(try await send(built)))
        return try unwrap(LetsDecideFFI.parse(model: model, request: req, response: res))
    }

    private func prefix(_ model: String) -> String {
        String(model.split(separator: "/", maxSplits: 1, omittingEmptySubsequences: false).first ?? "")
    }

    private func config(_ name: String) async throws -> [String: Any] {
        let own = byProvider[name] ?? ProviderSettings()
        // The provider's own headers win over shared ones with the same name.
        let ownNames = Set(own.headers.map { $0.0.lowercased() })
        let merged = headers.filter { !ownNames.contains($0.0.lowercased()) } + own.headers
        var config: [String: Any] = [:]
        config["api_key"] = own.apiKey
        config["access_token"] = try await own.accessToken?()
        config["base_url"] = own.baseUrl
        config["account_id"] = own.accountId
        config["headers"] = merged.map { [$0.0, $0.1] }
        config["options"] = own.options
        return config
    }

    private func send(_ built: [String: Any]) async throws -> HttpReply {
        let headers = (built["headers"] as? [[String]] ?? []).map { ($0[0], $0[1]) }
        guard let url = URL(string: built["url"] as? String ?? "") else {
            throw LetsDecideError(code: "InvalidRequest", message: "bad URL")
        }
        let http = Http(
            method: built["method"] as? String ?? "POST", url: url, headers: headers,
            body: Data((built["body"] as? String ?? "").utf8))
        return try await transport.send(http)
    }
}

private func replyJson(_ reply: HttpReply) -> [String: Any] {
    [
        "status": reply.status,
        "headers": reply.headers.map { [$0.0, $0.1] },
        "body": String(decoding: reply.body, as: UTF8.self),
    ]
}

private func json(_ v: [String: Any]) throws -> String {
    String(decoding: try JSONSerialization.data(withJSONObject: v), as: UTF8.self)
}

/// Names the provider in errors from a dialect, as built-ins do.
private func tagged<T>(_ name: String, _ f: () throws -> T) throws -> T {
    do {
        return try f()
    } catch var e as LetsDecideError {
        if e.details != nil && e.details?.provider == nil { e.details?.provider = name }
        throw e
    }
}

// The core returns {ok} or {error}; unwrap or throw.
private func unwrapAny(_ text: String) throws -> Any? {
    let out = try JSONSerialization.jsonObject(with: Data(text.utf8), options: .fragmentsAllowed) as? [String: Any] ?? [:]
    if let e = out["error"] as? [String: Any] { throw LetsDecideError(e) }
    return out["ok"]
}

private func unwrap(_ text: String) throws -> [String: Any] {
    try unwrapAny(text) as? [String: Any] ?? [:]
}
