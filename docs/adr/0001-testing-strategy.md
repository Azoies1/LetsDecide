# ADR 0001: Testing strategy and local testing

- Status: Proposed
- Date: 2026-11-19

## Context

The core is sans-IO (`build` and `parse` are pure), so most behaviour is cheap
to test offline. Today:

- One golden-fixture test runs 12 fixtures across OpenAI, Gemini and System One,
  plus 8 other integration tests and a compile-only doctest. `src/` has no
  unit tests.
- The JS binding (wasm) and the Python binding each have their own hand-written
  tests against a mocked transport.
- CI runs fmt, clippy and tests for all three.

Gaps:

- No coverage measurement, property tests or fuzzing.
- Fixtures are hand-written and never checked against real provider
  responses. `future-work.md` lists open questions about live shapes (Gemini
  safety-block shape, rate-limit body, top-level `model`).
- The bindings can drift from each other and from the core.
- Nothing tests the packaged artifacts (wheel, npm tarball, crate) the way a
  consumer installs them.
- Consumers have no documented way to test their own code without a key.

## Decision

Confidence comes mainly from offline tests. Live provider tests are the only
place real-world drift shows up, so they stay few, opt-in and out of the PR
gate.

### 1. Core correctness (offline, every PR)

- Unit tests in `src/` for validation and parsing edges: id rules and the
  64-character limit, Jev/Clef score cap of 10, MIME to kind mapping, Gemini
  limits, `retry_after` parsing, `Config` key redaction.
- Property tests (`proptest`): `validate()` and `parse()` never panic on
  arbitrary input; request JSON round-trips; probabilities in `Ok` answers are
  in [0,1]; errors never contain the api key or request body.
- Fuzz targets (`cargo-fuzz`) for `json::build` and `json::parse`, the
  untrusted-input surface for every binding. Short run in CI, longer nightly.
- Fixture hygiene: the golden test fails if a provider directory lacks the
  cases CONTRIBUTING requires (three question types, an error case, limits).
  Add error fixtures per provider: 401, 429 with and without `Retry-After`,
  5xx, malformed JSON, empty body.
- Coverage report (`cargo-llvm-cov`) on PRs with a soft floor of about 90% on
  `src/`, used to find untested branches.
- MSRV check and `cargo doc` with warnings denied.

### 2. Cross-binding conformance (offline, every PR)

`tests/fixtures/**` is the single source of truth. The JS and Python bindings
run the same fixtures and assert the same built request and parsed result.
Client behaviour (timeouts, retry caps, middleware) is tested against a fake
transport. A short conformance note under `spec/` lets future bindings (Swift,
Kotlin) reuse the suite.

### 3. Live contract tests (opt-in, nightly)

- `tests/live/`, gated by `LETSDECIDE_LIVE=1` and per-provider keys; skipped
  silently without keys so forks and consumers never fail.
- Nightly or manual dispatch only, never on PRs. Tiny prompts and a spend cap.
- Assert structure, not model opinions.
- Record responses, redact keys and ids, and commit them as new golden
  fixtures, so real responses become offline regression tests. This closes the
  open items in `future-work.md`.
- Failures open an issue (provider drift). They do not block merges.

### 4. Package and release smoke tests

Test what consumers install, not the checkout:

- Python: build the wheel, install in a fresh venv, run pytest and a type
  check of the README snippet (`py.typed`).
- JS: `npm pack`, install the tarball in a temp project, import on Node 20 and
  22, check `files` ships `pkg/` and the `.d.ts` files, run `tsc --noEmit`.
- Rust: `cargo package` and `cargo publish --dry-run`, then build the README
  example against the packaged crate.
- README snippets are compiled or run so docs cannot rot.
- Run in the release workflow before publishing, and on PRs touching bindings
  or packaging.

### 5. Local testing for contributors

- A `justfile` with one recipe per layer: `check` (fmt, clippy, `cargo test`;
  the fast default), `test-all` (adds wasm build, wheel build, JS and Python
  suites), `fuzz`, `live <provider>`, `record <provider>`.
- CI calls the same recipes, so local and CI runs match.
- CONTRIBUTING documents one-time setup: wasm target, `wasm-bindgen` at the
  pinned version, `maturin`, a venv. An optional pre-push hook runs `check`.

### 6. Local testing for consumers

Consumers need no API key to test their own code.

- Every client takes an injectable transport (`fetch` in JS, an `httpx` client
  in Python). Publish a "Testing your integration" guide showing how to stub it.
- Ship a small `testing` helper inside each package (`letsdecide.testing`,
  `letsdecide/testing`): a fake provider returning canned canonical responses,
  optionally with an error code or `retry_after`, built from the same fixtures.
- Rust consumers unit-test `build` and `parse` directly; no mocks needed.
- Optional `letsdecide-mock`: a localhost server speaking each provider's wire
  format from the fixtures, usable through the existing `base_url` override
  for end-to-end tests of retry and fallback middleware.
- Document how consumers can run the live suite with their own key to verify
  account and region before production.

## Rollout order

1. `justfile`, unit tests, error-path fixtures, coverage report.
2. Cross-binding conformance on shared fixtures.
3. Property tests and fuzz targets.
4. Package smoke tests in the release workflow.
5. `testing` helpers and the consumer guide.
6. Live nightly suite and recording (needs keys and a spend decision).

## Open questions

- Which provider keys are available, and what monthly spend cap applies to the
  nightly live suite?
- Ship `testing` helpers inside each package (leaning yes, to stay small) or as
  separate `letsdecide-testing` packages?
- Coverage: soft report only, or a failing gate?

## Consequences

- More CI jobs and a few dev dependencies (`proptest`, `cargo-fuzz`,
  `cargo-llvm-cov`, `just`), none of which ship to consumers.
- The `testing` helpers become public API that we must keep stable.
- Live tests add a recurring cost and secrets management; keeping them
  nightly-only bounds both.
- Real recorded fixtures replace guessed ones, so provider drift is caught by
  an issue instead of by users.
