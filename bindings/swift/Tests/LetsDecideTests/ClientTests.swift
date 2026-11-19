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
        let out = try await Client(apiKey: "k", transport: t).decide("gemini/gemini-decision-1", request)
        XCTAssertEqual(t.sent[0].url.absoluteString, "https://generativelanguage.googleapis.com/v1beta/decisions")
        XCTAssertTrue(t.sent[0].headers.contains { $0 == ("x-goog-api-key", "k") })
        let billing = (out["answers"] as! [String: Any])["billing"] as! [String: Any]
        XCTAssertEqual(billing["probability"] as! Double, 0.9, accuracy: 1e-6)
        XCTAssertEqual(out["input_tokens"] as? Int, 7)
    }

    func testErrorsAreTyped() async throws {
        let t = Recorder(429, "{}", headers: [("Retry-After", "2")])
        do {
            _ = try await Client(apiKey: "k", transport: t).decide("jev/jev-latest", request)
            XCTFail("expected an error")
        } catch let e as LetsDecideError {
            XCTAssertEqual(e.code, "RateLimited")
            XCTAssertTrue(e.retryable)
            XCTAssertEqual(e.retryAfterMs, 2000)
        }
    }
}
