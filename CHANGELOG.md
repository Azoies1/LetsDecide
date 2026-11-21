# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.0] - 2026-11-21

### Added
- Audio, video and PDF input for Gemini (`media`, replacing `images`).
- Python binding (PyO3, sync and async clients).
- JS middleware: `retry`, `fallback`.
- Shared JSON layer in the core (`letsdecide::json`).
- `ErrorCode::as_str`, the stable wire name of an error code.
- JS `retry({ maxMs })` caps every wait, including a server `Retry-After`
  (default 30 s).
- README and LICENSE in the npm and PyPI packages.

### Changed
- **Breaking:** `images` is now `media`.
- Python clients default to a 60 s timeout (httpx's own default is 5 s).

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
[0.2.0]: https://github.com/azoies1/letsdecide/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/azoies1/letsdecide/releases/tag/v0.1.0
