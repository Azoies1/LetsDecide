# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.0] - 2026-11-21

### Added
- `fanOut` (JS) and `fan_out` (Python middleware): ask several models the same
  request at once and get every raw result back, keyed by model. Supports
  per-model `enabled`, a deadline, and any composed `decide`. Nothing is
  merged. See `docs/concurrent-models.md`.
- Custom engines' extra response fields are now kept after the response check.
- Kotlin and Swift bindings over UniFFI (`bindings/uniffi`, `bindings/kotlin`, `bindings/swift`).
- Kotlin and Swift clients: `dialects` and `replace`, per-provider `options`,
  typed error `details` (`ErrorDetails`), and the helpers `dialect`,
  `resolveDialect`, `validate`, `check`, `classifyHttp`, `capabilities` and
  `BUILTIN`, matching JS and Python.
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
- JS and Python clients take custom `providers` (codecs: `build` + `parse`)
  and `engines` (`decide`), with `replace` to override a built-in name.
  Exceptions from user code become `ProviderError`; the original is kept on
  `.original`, out of loggers and tracebacks.
- `mockEngine` / `mock_engine` for tests, and helpers `validate`, `check`,
  `classifyHttp` / `classify_http`, `capabilities`, `BUILTIN`.
- Conformance kit: `letsdecide/conformance` (`checkProvider`) and
  `letsdecide.conformance` (`check_provider`), over the golden fixture format.
- `byProvider` / `by_provider` (JS, Python) and `ProviderSettings` (Kotlin,
  Swift): settings per provider prefix; a provider only sees its own entry;
  any value may be a function, called per request. Client-wide `headers` go to
  every provider (ADR 0014).
- Engines get a `decide` callback to call other models through the client,
  capped at `MAX_DEPTH` nested calls; `client.capabilities(model)` for any
  model, built-in or custom.
- Guide: `docs/custom-providers.md`. Core `json::builtins()`.
- Plan for custom providers and local models
  (`docs/plan-custom-and-local-providers.md`).

- **Dialects**: providers described as data (`Dialect`; JS/Python
  `dialects`), for APIs that are System One or OpenAI variants. A dialect
  `extends` a built-in and overrides what differs (endpoint with `{model}`,
  `{account_id}` and `{options.<name>}` placeholders, auth header with the
  scheme inferred, input field, media encoding, envelope, block codes,
  capabilities), e.g. to patch a vendor's API change without a release
  (`replace` for built-in names). Every built-in is now a dialect in
  `spec/dialects/*.json`; fixtures unchanged. Helpers: `dialect(name)`,
  `resolveDialect` / `resolve_dialect`; core `json::dialect`,
  `resolve_dialect`, `build_dialect`, `parse_dialect` (ADR 0015,
  `docs/dialects.md`).
- `ErrorCode::as_str`, the stable wire name of an error code.
- JS `retry({ maxMs })` caps every wait, including a server `Retry-After`
  (default 30 s).
- README and LICENSE in the npm and PyPI packages.

### Changed
- **Breaking:** `SystemOne` and `OpenAi` are replaced by `Dialect`
  (`Dialect::builtin("jev")`, ...). The OpenAI built-in now honours a
  `base_url` setting.
- **Breaking:** `images` is now `media`.
- **Breaking:** `Provider` has a required `capabilities(model)` method.
- **Breaking:** clients take credentials and endpoints only per provider
  (`byProvider`). A top-level `apiKey`, `accessToken`, `baseUrl`, `accountId`
  or `options` is refused with `InvalidRequest` (`invalid_config`), so a key
  can never be sent to another provider by fallback, hedging or panels.
- **Breaking:** `Error` has a `details` field; `DecisionResponse` has
  `probability_source`.
- Built-in responses are now checked; a malformed reply is `BadResponse`.
- Scope: the library stays a thin access layer. The planned label scorer,
  `letsdecide-serve` sidecar, bundled engines, `escalate` middleware and
  Kotlin/Swift engine slots are dropped (ADR 0016).
- Python clients default to a 60 s timeout (httpx's own default is 5 s).

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

[Unreleased]: https://github.com/azoies1/letsdecide/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/azoies1/letsdecide/releases/tag/v0.3.0
