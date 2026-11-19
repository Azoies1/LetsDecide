# Concurrent models (proposal)

Status: **plan, not built.** Ask several decision models the same request at
once, turn each on or off, and set a few rules for how their answers combine.

## The idea in one line

A **panel** is a named group of models that you call like a single model:

```js
await decide("panel/moderation", request); // same call as "jev/jev-latest"
```

This keeps the library's promise that you switch with one string. Going from
one model to an ensemble is a config change. Your call sites stay the same.

## What exists vs. what's missing

| Need | Today | Gap |
|---|---|---|
| Call N models | `fallback` (one after another) | No parallel fan-out |
| Different key per provider | `Client` has one `api_key` | **Blocker**: needs a key per provider |
| Combine answers | none | Pure logic, so it belongs in the core |
| Cancel the losers | `opts.signal` is threaded through | Needs a child `AbortController` per member |

## Design

### 1. Split: pure merge in the core, fan-out in the bindings

This follows the existing sans-IO rule:

- **Core (Rust):** `merge(request, [(model, response | error, weight)], rules)` returns
  a merged response plus agreement data. It is pure, has golden tests, and every
  binding gets it for free through `json::merge`.
- **Bindings (JS first, then Python):** `panel(...)` middleware sends the calls
  concurrently (`Promise.allSettled` / `asyncio.gather`) and passes the results to `merge`.

### 2. Config is plain data, never functions

Because the config is plain data, it can come from a JSON file, an environment
variable or a feature-flag service, and you can change it at runtime without a deploy.

```json
{
  "panels": {
    "moderation": {
      "mode": "vote",
      "members": [
        { "model": "jev/jev-latest" },
        { "model": "gemini/gemini-decision-1", "weight": 2 },
        { "model": "openai/gpt-6-luna", "enabled": false }
      ],
      "rules": { "quorum": 2, "timeout_ms": 3000, "on_disagree": "flag" }
    }
  },
  "keys": { "jev": "...", "gemini": "...", "openai": "..." }
}
```

- **On/off:** set `enabled: false` on a member. A panel with one enabled member
  behaves exactly like calling that model directly, so concurrency is off.
- **Per-model settings:** `weight`, `timeout_ms`, plus an optional `config`
  override (`base_url`, `account_id`).
- **Live toggles:** `panel()` accepts either an object or a getter
  `() => config`, and reads it on every call.

### 3. Modes (four, no more)

| Mode | Behaviour | Typical use |
|---|---|---|
| `all` | Waits for every member and returns each raw response | Comparing models, debugging |
| `race` | Returns the first success and aborts the rest | Lower latency (hedging) |
| `vote` | Waits for members, merges answers per question | Accuracy, robustness |
| `cascade` | Calls members in order and stops once the answer is confident | Accuracy at close to single-model cost |

`shadow` (answer from the primary, others in the background for evaluation) is
`all` with a primary member. It lands with M5.

### 4. Rules (a small fixed set, not a DSL)

| Rule | Meaning | Default |
|---|---|---|
| `quorum` | Minimum successful members, else throw the last error | `1` |
| `timeout_ms` | Panel deadline; merge whatever has arrived | none |
| `on_disagree` | `flag` (mark it in output) · `error` · `primary` (use the first member) | `flag` |
| `escalate_below` | `cascade` only: call the next member when the margin is under this | `0.8` |

Members that return `Unsupported` (for example Clef with more than 64
questions) are **skipped and recorded**. They do not count as failures, which
matches how `fallback` already treats them.

### 5. Merging honestly

The library's own non-goal says confidence can't be compared across providers,
so the **default vote is scale-free**:

- `yesno`: majority of `p >= 0.5`. The merged `probability` is the weighted
  share of yes votes.
- `choice` / `score`: weighted majority of the argmax option or level id. Ids are
  yours, so they compare across providers. Score `value` is never averaged,
  because OpenAI and the System One family use different scales.
- `Refused` answers abstain, and a question where every member refused stays
  `Refused`.
- Averaging probabilities is opt-in (`"merge": "mean"`) and documented as
  "only meaningful within one provider family".

### 6. Output stays a `DecisionResponse`

```js
res.answers.flagged          // merged answer, same shape as one model
res.agreement.flagged        // 0..1, weighted share that agreed with the winner
res.disagreed                // ["flagged"] when on_disagree = "flag"
res.panel["jev/jev-latest"]  // raw response or { error }, ms elapsed
```

Because the shape is unchanged, everything downstream keeps working:
`compose(client, panel(cfg), retry())` retries **each member** separately, and
`fallback` can sit inside one member.

## Plan

Each step can ship and be useful on its own.

1. **Keys per provider** (`Client({ keys: { jev, gemini } })`, falling back to
   `apiKey`). Small, and needed for everything else. JS and Python.
2. **`panel` middleware in JS with `all` and `race`**, `enabled` flags, getter
   config, and abort of losers. No core change yet.
3. **Core `merge` and `vote` mode** with `quorum`, `on_disagree`, `agreement`.
   Golden tests: unanimous, split, all refused, one `Unsupported`, below quorum.
4. **`cascade`** with `escalate_below` (it reuses `merge`).
5. **Python parity** for steps 2–4 (`AsyncClient` only; sync gets `all`
   sequentially, or nothing).
6. **`shadow`**, folded into M5 evaluation.

Rough size: core about 120 lines (`src/merge.rs`), JS about 80 lines, docs and tests.

## Deliberately not doing

- A general rule engine or expression language. Four rules cover the cases
  people actually have.
- Learned or auto-tuned weights and calibration. That belongs to M5 and must
  never be silent.
- Cost-based routing. Add it with `spec/prices.json` if that ever exists.
- Concurrency inside the Rust core. It stays sans-IO.

## Open questions

- Default mode when `mode` is omitted: `vote`, or `all` as the least surprising?
- Per-question routing (send question X only to Gemini): useful, or premature?
- Should `cascade` measure "confident" as top-probability margin, or the
  provider's `confidence` when present?
