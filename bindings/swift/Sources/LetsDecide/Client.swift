import Foundation
import LetsDecideFFI

/// A typed error from any provider. Never carries keys or request bodies.
public struct LetsDecideError: Error, Equatable {
    public let code: String
    public let message: String
    public let status: Int?
    public let retryAfterMs: Int?
    public let retryable: Bool
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

/// One request shape for every decision API. Requests and responses are JSON
/// dictionaries in the canonical shape (see the README).
public final class Client: Sendable {
    let apiKey: String?
    let accessToken: (@Sendable () async throws -> String)?
    let baseUrl: String?
    let accountId: String?
    let transport: Transport

    /// - Parameter accessToken: OAuth token provider (Clef), called on every
    ///   request so the host can refresh it. Wins over `apiKey`.
    public init(
        apiKey: String? = nil,
        accessToken: (@Sendable () async throws -> String)? = nil,
        baseUrl: String? = nil,
        accountId: String? = nil,
        transport: Transport = URLSessionTransport()
    ) {
        self.apiKey = apiKey
        self.accessToken = accessToken
        self.baseUrl = baseUrl
        self.accountId = accountId
        self.transport = transport
    }

    /// `model`: `"provider/model"`, e.g. `"clef/clef-flash"`. Throws `LetsDecideError`.
    public func decide(_ model: String, _ request: [String: Any]) async throws -> [String: Any] {
        let req = try json(request)
        var config: [String: Any] = [:]
        config["api_key"] = apiKey
        config["access_token"] = try await accessToken?()
        config["base_url"] = baseUrl
        config["account_id"] = accountId
        let built = try unwrap(LetsDecideFFI.build(model: model, request: req, config: try json(config)))
        let headers = (built["headers"] as? [[String]] ?? []).map { ($0[0], $0[1]) }
        guard let url = URL(string: built["url"] as? String ?? "") else {
            throw LetsDecideError(code: "InvalidRequest", message: "bad URL", status: nil, retryAfterMs: nil, retryable: false)
        }
        let http = Http(
            method: built["method"] as? String ?? "POST", url: url, headers: headers,
            body: Data((built["body"] as? String ?? "").utf8))
        let reply = try await transport.send(http)
        let res: [String: Any] = [
            "status": reply.status,
            "headers": reply.headers.map { [$0.0, $0.1] },
            "body": String(decoding: reply.body, as: UTF8.self),
        ]
        return try unwrap(LetsDecideFFI.parse(model: model, request: req, response: try json(res)))
    }

    private func json(_ v: [String: Any]) throws -> String {
        String(decoding: try JSONSerialization.data(withJSONObject: v), as: UTF8.self)
    }

    // The core returns {ok} or {error}; unwrap or throw.
    private func unwrap(_ text: String) throws -> [String: Any] {
        let out = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] ?? [:]
        if let e = out["error"] as? [String: Any] {
            throw LetsDecideError(
                code: e["code"] as? String ?? "BadResponse",
                message: e["message"] as? String ?? "",
                status: e["status"] as? Int,
                retryAfterMs: e["retry_after_ms"] as? Int,
                retryable: e["retryable"] as? Bool ?? false)
        }
        return out["ok"] as? [String: Any] ?? [:]
    }
}
