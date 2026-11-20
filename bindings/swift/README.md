# LetsDecide (Swift)

One request shape for every decision API (Jev, Clef, OpenAI, Gemini). The Rust
core builds and parses through UniFFI; `URLSession` sends. Requests and
responses are `[String: Any]` in the canonical shape.

```swift
import LetsDecide

let client = Client(apiKey: "...") // also: accessToken: { try await fresh() }, baseUrl:, accountId:, transport:
do {
    let out = try await client.decide("jev/jev-latest", [
        "input": "Great product, arrived late.",
        "questions": ["positive": ["kind": "yesno", "instructions": "Is the review positive?"]],
    ])
    print(out["answers"]!)
} catch let e as LetsDecideError {
    print(e.code, e.status as Any, e.retryable, e.retryAfterMs as Any)
}
```

Pass your own `Transport` for proxies, pinning or tests.

## Build and test (macOS)

Needs Rust and Xcode. `build.sh` builds the core for macOS and iOS (device and
simulator), packs `LetsDecideFFIFFI.xcframework` and generates the glue in
`Sources/LetsDecideFFI`.

```sh
./build.sh
swift test
```

Status: the Swift glue is generated from the same UniFFI crate as Kotlin, and
CI builds and tests it on macOS. It has not been compiled outside CI yet.
