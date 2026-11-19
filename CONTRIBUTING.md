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

0. If the provider speaks the System One shape, add a preset to `src/systemone.rs`
   instead of a new file.
1. Add `src/<provider>.rs` implementing `Provider` (`build`, `parse`).
   - Pure: no I/O, no clock, no global state.
   - Validate with `DecisionRequest::validate()`, then check provider limits.
   - Return `Unsupported` for what the provider can't do. Never degrade silently.
2. Register it in `resolve` (`src/provider.rs`).
3. Add golden fixtures in `tests/fixtures/<provider>/`: all three question
   types, a refusal or error case, and the limits. Each file holds the canonical
   request, the exact HTTP request, a recorded response and the expected result.
4. Add a row to the README model table.

Nothing else should change. If it does, the design needs discussion first.

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
   `bindings/python/Cargo.toml`, `bindings/js/package.json` and
   `bindings/python/pyproject.toml`, then run `cargo check` to refresh `Cargo.lock`.
2. Move the `[Unreleased]` entries in `CHANGELOG.md` under the new version and
   update the compare links.
3. Merge, then `git tag v<version> && git push origin v<version>`. The tag must
   match all five versions or the release stops before building.

Publishing is opt-in per registry and needs credentials set up once. Where to
create them, the first-release steps and troubleshooting are in
[docs/releasing.md](docs/releasing.md). A published version number can't be reused (a
bad release can only be yanked or deprecated), so check the release run first.
