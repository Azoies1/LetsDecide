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
| [0007](0007-oauth-token-in-config-host-refreshes.md) | OAuth token in `Config`; host refreshes | Accepted |
| [0008](0008-provider-registry-free-names.md) | Provider registry; free names, a clash is an error | Accepted |
| [0009](0009-capabilities-and-response-check.md) | Capabilities as data; every response checked | Accepted |
| [0010](0010-safe-error-details.md) | Structured error details that are safe to log | Accepted |
| [0011](0011-local-models-label-scoring.md) | Local models read label probabilities; no text parsing | Accepted |
| [0012](0012-no-default-local-model.md) | No default local model | Accepted |
| [0013](0013-engine-packaging.md) | Model runtimes ship in separate packages | Accepted |
| [0014](0014-credentials-per-provider.md) | Credentials and endpoints are set per provider only | Accepted |
| [0015](0015-dialects.md) | Dialects: providers as data, with named strategies | Accepted |
