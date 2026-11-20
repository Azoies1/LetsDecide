# letsdecide (Kotlin/JVM)

One request shape for every decision API (Jev, Clef, OpenAI, Gemini). The Rust
core builds and parses through UniFFI; `java.net.http` sends. Requests and
responses are `org.json.JSONObject`s in the canonical shape.

```kotlin
import letsdecide.Client
import letsdecide.LetsDecideException
import org.json.JSONObject

val client = Client(apiKey = "...") // also: accessToken = { fresh() }, baseUrl, accountId, transport
try {
    val out = client.decide("jev/jev-latest", JSONObject("""{
        "input": "Great product, arrived late.",
        "questions": {"positive": {"kind": "yesno", "instructions": "Is the review positive?"}}}"""))
    println(out.getJSONObject("answers"))
} catch (e: LetsDecideException) {
    println("${e.code} ${e.status} ${e.retryable} ${e.retryAfterMs}")
}
```

`decide` blocks; call it from a background thread or `Dispatchers.IO`. Pass
your own `Transport` for proxies, pinning or tests. Android: build the library
per ABI with `cargo ndk` and package it under `jniLibs`; untested so far.

## Build and test

Needs Rust, a JDK (17+) and Gradle.

```sh
./gen.sh      # builds the Rust library, generates the UniFFI glue
gradle test
```
