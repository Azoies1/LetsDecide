# Architecture decision records

Short, numbered records of decisions that shape the library. Each one says what
was decided, why, and what it costs. Add a new file for a new decision; do not
rewrite old ones. Supersede them with a later record instead.

Format: copy [0000-template.md](0000-template.md).

| # | Decision | Status |
|---|---|---|
| [0001](0001-sans-io-core.md) | Sans-IO core | Accepted |
| [0002](0002-strict-id-rule.md) | Strict id rule `[a-z0-9_]{1,64}` | Accepted |
| [0003](0003-media-replaces-images.md) | `media` replaces `images` (breaking) | Accepted |
| [0004](0004-score-is-level-index.md) | Scores are 0-based level indexes | Accepted |
| [0005](0005-errors-over-degradation.md) | Unsupported features error, never degrade | Accepted |
| [0006](0006-keys-in-memory-only.md) | Keys live in memory only | Accepted |
