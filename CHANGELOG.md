# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Audio, video and PDF input for Gemini (`media`, replacing `images`).
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
- Python wheel builds with current maturin (`pyo3/extension-module`).

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
