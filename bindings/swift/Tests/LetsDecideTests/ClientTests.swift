import Foundation
import XCTest

@testable import LetsDecide

final class Recorder: Transport, @unchecked Sendable {
    let status: Int
    let headers: [(String, String)]
    let body: String
    var sent: [Http] = []

    init(_ status: Int, _ body: String, headers: [(String, String)] = []) {
        self.status = status
        self.body = body
        self.headers = headers
    }

    func send(_ http: Http) async throws -> HttpReply {
        sent.append(http)
        return HttpReply(status: status, headers: headers, body: Data(body.utf8))
    }
}

let request: [String: Any] = [
    "input": "Charged twice.",
    "questions": ["billing": ["kind": "yesno", "instructions": "Is this about billing?"]],
]

final class ClientTests: XCTestCase {
    func testBuildsSendsAndParses() async throws {
        let t = Recorder(200, #"{"answers": {"billing": {"noul": 0.9}}, "usage": {"total_input_tokens": 7}}"#)
        let out = try await Client(byProvider: ["gemini": ProviderSettings(apiKey: "k")], transport: t).decide("gemini/gemini-decision-1", request)
        XCTAssertEqual(t.sent[0].url.absoluteString, "https://generativelanguage.googleapis.com/v1beta/decisions")
        XCTAssertTrue(t.sent[0].headers.contains { $0 == ("x-goog-api-key", "k") })
        let billing = (out["answers"] as! [String: Any])["billing"] as! [String: Any]
        XCTAssertEqual(billing["probability"] as! Double, 0.9, accuracy: 1e-6)
        XCTAssertEqual(out["input_tokens"] as? Int, 7)
    }

    func testErrorsAreTyped() async throws {
        let t = Recorder(429, "{}", headers: [("Retry-After", "2")])
        do {
            _ = try await Client(byProvider: ["jev": ProviderSettings(apiKey: "k")], transport: t).decide("jev/jev-latest", request)
            XCTFail("expected an error")
        } catch let e as LetsDecideError {
            XCTAssertEqual(e.code, "RateLimited")
            XCTAssertTrue(e.retryable)
            XCTAssertEqual(e.retryAfterMs, 2000)
        }
    }

    func testEachProviderOnlySeesItsOwnSettings() async throws {
        let t = Recorder(401, "{}")
        let client = Client(
            byProvider: ["gemini": ProviderSettings(apiKey: "g")],
            headers: [("x-trace", "t")],
            transport: t)
        for model in ["gemini/gemini-decision-1", "jev/jev-latest"] {
            do {
                _ = try await client.decide(model, request)
                XCTFail("expected an error")
            } catch is LetsDecideError {}
        }
        XCTAssertTrue(t.sent[0].headers.contains { $0 == ("x-goog-api-key", "g") })
        XCTAssertFalse(t.sent[1].headers.contains { $0.0.lowercased() == "authorization" })
        XCTAssertFalse(t.sent[1].headers.contains { $0.1 == "g" })
        XCTAssertTrue(t.sent.allSatisfy { h in h.headers.contains { $0 == ("x-trace", "t") } })
    }

    let jevBody = #"{"answers": {"billing": {"noul": 0.9}}}"#

    func testDialectsRouteByPrefixWithOptions() async throws {
        let t = Recorder(200, jevBody)
        let client = try Client(
            byProvider: ["acme": ProviderSettings(apiKey: "k", options: ["region": "eu"])],
            transport: t,
            dialects: ["acme": [
                "extends": "systemone", "base_url": "https://api.acme.ai",
                "path": "/v1/{options.region}/decide", "auth": ["header": "api-key", "required": true],
            ]])
        let out = try await client.decide("acme/acme-1", request)
        XCTAssertEqual(t.sent[0].url.absoluteString, "https://api.acme.ai/v1/eu/decide")
        XCTAssertTrue(t.sent[0].headers.contains { $0 == ("api-key", "k") })
        let billing = (out["answers"] as! [String: Any])["billing"] as! [String: Any]
        XCTAssertEqual(billing["probability"] as! Double, 0.9, accuracy: 1e-6)
        XCTAssertNotNil(try client.capabilities("acme/acme-1")["media"])
    }

    func testDialectErrorsNameTheProvider() async throws {
        let client = try Client(
            transport: Recorder(200, "{}"),
            dialects: ["acme": ["extends": "systemone", "base_url": "https://api.acme.ai", "auth": ["required": true]]])
        do {
            _ = try await client.decide("acme/acme-1", request)
            XCTFail("expected an error")
        } catch let e as LetsDecideError {
            XCTAssertEqual(e.details?.provider, "acme")
            XCTAssertEqual(e.details?.reason, "missing_config")
        }
    }

    func testBadDialectsAndBuiltinNames() throws {
        XCTAssertThrowsError(try Client(dialects: ["acme": ["extends": "jev", "pathh": "/x"]])) { e in
            XCTAssertEqual((e as? LetsDecideError)?.details?.provider, "acme")
        }
        XCTAssertThrowsError(try Client(dialects: ["gemini": ["extends": "gemini"]])) { e in
            XCTAssertEqual((e as? LetsDecideError)?.details, ErrorDetails(reason: "name_taken", provider: "gemini"))
        }
        XCTAssertNoThrow(try Client(dialects: ["gemini": ["extends": "gemini"]], replace: ["gemini"]))
    }

    func testTopLevelHelpers() throws {
        XCTAssertTrue(BUILTIN.contains("gemini"))
        XCTAssertEqual(try dialect("jev")["family"] as? String, "systemone")
        XCTAssertEqual(try resolveDialect(["extends": "jev"])["base_url"] as? String, "https://api.typesafe.ai")
        XCTAssertNoThrow(try validate(request))
        let e = try classifyHttp(HttpReply(status: 429, headers: [("Retry-After", "3")], body: Data("{}".utf8)))
        XCTAssertEqual(e.code, "RateLimited")
        XCTAssertEqual(e.retryAfterMs, 3000)
    }
}
