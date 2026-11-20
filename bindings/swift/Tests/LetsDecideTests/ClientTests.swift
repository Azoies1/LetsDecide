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
}
