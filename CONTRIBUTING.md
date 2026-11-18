# Contributing

Keep it small. If a feature isn't needed by a real provider or user today, it
doesn't belong yet (YAGNI).

## Setup

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same three.

## Adding a provider

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
