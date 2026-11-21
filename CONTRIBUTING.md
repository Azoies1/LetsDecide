# Contributing

Keep it small. If a feature isn't needed by a real provider or user today, it
doesn't belong yet (YAGNI).

## Setup

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps   # the README is the crate docs
```

CI runs these plus the two bindings below.

JavaScript (`bindings/js`) needs the WASM target and the `wasm-bindgen-cli`
version CI pins:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked
cd bindings/js && npm run build && npm test
```

Python (`bindings/python`):

```sh
cd bindings/python
python -m venv .venv && . .venv/bin/activate
pip install maturin pytest pytest-asyncio httpx
maturin develop && pytest
```

## Adding a provider

0. If it speaks the System One or OpenAI shape, it's a **dialect**: add
   `spec/dialects/<provider>.json` (see [docs/dialects.md](docs/dialects.md))
   and list it in `BUILTIN_SOURCES` (`src/dialect.rs`). No new code. If it
   needs a new media encoding or similar that a second provider would also
   use, add it as a named strategy in `src/dialect.rs`.
1. Otherwise add `src/<provider>.rs` implementing `Provider` (`capabilities`, `build`, `parse`).
   - Pure: no I/O, no clock, no global state.
   - Declare limits and media in `capabilities()`; call
     `self.capabilities(model).check(req)?` first in `build`, then `apply_headers` last.
   - Return `Unsupported` for what the provider can't do, with `Details` and a
     `Reason`. Never degrade silently.
   - Error messages and details: names, ids, counts and MIME types only.
     Never input, descriptions, bodies, keys or config values.
2. Add its prefix to `BUILTIN` (`src/provider.rs`) and, for a codec, to
   `builtin()`. List it in the changelog: new built-in names can clash with
   users' custom ones.
3. Add golden fixtures in `tests/fixtures/<provider>/`: all three question
   types, a refusal or error case, and the limits. Each file holds the canonical
   request, the exact HTTP request, a recorded response and the expected result.
4. Add a row to the README model table.

Nothing else should change. If it does, the design needs discussion first.

Not ready to upstream it? Users can register it themselves, as a dialect or
a codec (see the README's "Custom providers").

## A provider changed its API

Fix its `spec/dialects/<provider>.json` (and fixtures). Until a release ships,
users can patch it themselves: `dialects: { gemini: { extends: "gemini", ... } }`
with `replace: ["gemini"]`.

## Conventions

- Canonical names are neutral (`yesno`, not `noul` or `predicate`).
- Probabilities pass through as the provider returned them; never rescale.
- Never put keys or request bodies in errors or logs.
- Comments are short and say *why*, not *what*.

## Commits and PRs

One logical change per PR. Include tests. Describe the behaviour change, not the diff.

## Releasing

One tag publishes all three packages (crates.io, npm, PyPI) under the same
version. Pushing a tag builds and checks everything first, and only then
publishes, so a failed build never leaves a half-published release.

1. Set the new version in `Cargo.toml`, `bindings/wasm/Cargo.toml`,
   `bindings/python/Cargo.toml`, `bindings/uniffi/Cargo.toml`,
   `bindings/js/package.json`, `bindings/python/pyproject.toml` and
   `bindings/kotlin/build.gradle.kts`, then run `cargo check` to refresh `Cargo.lock`.
2. Move the `[Unreleased]` entries in `CHANGELOG.md` under the new version and
   update the compare links.
3. Merge, then `git tag v<version> && git push origin v<version>`. The tag must
   match all seven versions or the release stops before building.

Publishing is opt-in per registry and needs credentials set up once. Where to
create them, the first-release steps and troubleshooting are in
[docs/releasing.md](docs/releasing.md). A published version number can't be reused (a
bad release can only be yanked or deprecated), so check the release run first.
