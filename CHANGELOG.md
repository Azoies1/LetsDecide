# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Panels, core part: `merge` (Rust) and `json::merge` (WASM, Python, UniFFI)
  combine several models' answers by weighted vote or same-source mean, with
  `quorum`, `agreement` and `disagreed`. New reasons `below_quorum` and
  `mixed_probability_source`; new `probability_source` value `vote`. The
  panel engine itself follows. See `docs/concurrent-models.md`.
- Kotlin and Swift bindings over UniFFI (`bindings/uniffi`, `bindings/kotlin`, `bindings/swift`).
- Middleware (JS and async Python): `hedge`, `split`, `shadow` + `compare`,
  `calibrate` + `fitIsotonic` + `brierScore`, `observe`. Python also gets
  `retry` and `fallback`.
- OpenAI takes public http(s) image URLs in `media[].uri`.
- `confidence_source` on choice and score answers.
- `provider_extras` on requests.
- Cost estimates: `estimate_cost_usd`, and `estimated_cost_usd` on binding
  responses, from `spec/prices.json`.
- The JS package root now re-exports the middleware.
- Authentication: OAuth `access_token` for Clef (string, or a function in JS and Python that is called per request so tokens can refresh); Gemini `api_key` is now checked up front. Both fail with `InvalidRequest` before sending if no credential is set.
- Audio, video and document input for Gemini (`media`, replacing `images`), with `uri` and per-media `options`.
- Gemini yes/no descriptions and `block_reason` on refused responses.
- Python binding (PyO3, sync and async clients).
- JS middleware: `retry`, `fallback`.
- Shared JSON layer in the core (`letsdecide::json`).
- `Registry`: register custom providers by name; taken names are an error
  unless replaced. `Registry::build/parse` check capabilities first and the
  response after.
- `Capabilities` (limits, accepted media, question kinds, probability
  source, locality) on every provider; `json::capabilities`.
- `DecisionResponse::check` and `probability_source` on every response.
- Error `details` with a stable `reason`; new `ProviderError` code.
- `Config.headers` (replace same-named adapter headers; injection rejected)
  and `Config.options`. Both in the JS and Python clients.
- JSON entry points for custom providers: `validate`, `check`,
  `classify_http`, `capabilities` (Rust, WASM, Python).
- `http_error` and `apply_headers` are public.
- Plan for custom providers and local models
  (`docs/plan-custom-and-local-providers.md`).

### Changed
- **Breaking:** `images` is now `media`.
- **Breaking:** `Provider` has a required `capabilities(model)` method.
- **Breaking:** `Error` has a `details` field; `DecisionResponse` has
  `probability_source`.
- Built-in responses are now checked; a malformed reply is `BadResponse`.

### Fixed
- Error messages no longer quote values from request or response JSON
  (serde's `invalid type: string "…"`).
- `Config`'s `Debug` no longer prints a `base_url` query string.

## [0.2.0]

### Added
- TypeScript/WebAssembly binding (`bindings/js`) over the same sans-IO core.

## [0.1.0]

### Added
- Canonical decision request/response model and sans-IO `Provider` core.
- System One adapter with Jev, Clef and custom-server presets.
- OpenAI and Gemini adapters.
- Image input.
- Golden fixtures for every provider.

[Unreleased]: https://github.com/azoies1/letsdecide/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/azoies1/letsdecide/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/azoies1/letsdecide/releases/tag/v0.1.0
