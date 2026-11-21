# letsdecide (Kotlin/JVM)

One request shape for every decision API (Jev, Clef, OpenAI, Gemini). The Rust
core builds and parses through UniFFI; `java.net.http` sends. Requests and
responses are `org.json.JSONObject`s in the canonical shape.

```kotlin
import letsdecide.Client
import letsdecide.LetsDecideException
import letsdecide.ProviderSettings
import org.json.JSONObject

// Settings per provider: a key only ever goes to its own provider.
val client = Client(byProvider = mapOf("jev" to ProviderSettings(apiKey = "..."))) // also: accessToken = { fresh() }, baseUrl, accountId, headers; shared headers, transport
try {
    val out = client.decide("jev/jev-latest", JSONObject("""{
        "input": "Great product, arrived late.",
        "questions": {"positive": {"kind": "yesno", "instructions": "Is the review positive?"}}}"""))
    println(out.getJSONObject("answers"))
} catch (e: LetsDecideException) {
    println("${e.code} ${e.status} ${e.retryable} ${e.retryAfterMs}")
}
```

`decide` blocks; call it from a background thread or `Dispatchers.IO`.

Errors carry `details` (`ErrorDetails`: `reason`, `provider`, `question`, `id`,
`limit`, `actual`, `mimeType`, `field`), safe to log: never keys or content.

### Dialects

Providers described as data ([docs/dialects.md](../../docs/dialects.md)).
They are validated when the client is made.

```kotlin
val client = Client(
    byProvider = mapOf("acme" to ProviderSettings(apiKey = "...", options = mapOf("region" to "eu"))),
    dialects = mapOf("acme" to JSONObject("""{"extends": "jev", "base_url": "https://api.acme.ai",
        "path": "/v1/{options.region}/decisions"}""")),
    // replace = setOf("gemini") to override a built-in
)
client.decide("acme/acme-decide-1", request)
```

Top-level helpers: `dialect(name)`, `resolveDialect(spec)`, `capabilities(model)`,
`validate(request, capabilities)`, `check(request, response)`, `classifyHttp(reply)`, `BUILTIN`. Pass
your own `Transport` for proxies, pinning or tests. Android: build the library
per ABI with `cargo ndk` and package it under `jniLibs`; untested so far.

## Build and test

Needs Rust, a JDK (17+) and Gradle.

```sh
./gen.sh      # builds the Rust library, generates the UniFFI glue
gradle test
```
