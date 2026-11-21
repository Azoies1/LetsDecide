# LetsDecide (Swift)

One request shape for every decision API (Jev, Clef, OpenAI, Gemini). The Rust
core builds and parses through UniFFI; `URLSession` sends. Requests and
responses are `[String: Any]` in the canonical shape.

```swift
import LetsDecide

// Settings per provider: a key only ever goes to its own provider.
let client = Client(byProvider: ["jev": ProviderSettings(apiKey: "...")])
// ProviderSettings also takes accessToken: { try await fresh() }, baseUrl:, accountId:, headers:.
// Client also takes shared headers: and transport:.
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

Pass your own `Transport` for proxies, pinning or tests. Errors carry
`details` (`ErrorDetails`: `reason`, `provider`, `question`, `id`, `limit`,
`actual`, `mimeType`, `field`), safe to log: never keys or content.

### Dialects

Providers described as data ([docs/dialects.md](../../docs/dialects.md)). The
client initialiser that takes `dialects:` validates them and throws on a bad one.

```swift
let client = try Client(
    byProvider: ["acme": ProviderSettings(apiKey: "...", options: ["region": "eu"])],
    dialects: ["acme": ["extends": "jev", "base_url": "https://api.acme.ai",
                        "path": "/v1/{options.region}/decisions"]])
    // replace: ["gemini"] to override a built-in
_ = try await client.decide("acme/acme-decide-1", request)
```

Free functions: `dialect(_:)`, `resolveDialect(_:)`, `capabilities(_:)`,
`validate(_:capabilities:)`, `check(_:_:)`, `classifyHttp(_:)`, `BUILTIN`.

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
