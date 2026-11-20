# Concurrent models (proposal)

Status: **plan, not built.** Ask several decision models the same request at
once, turn each on or off, and set a few rules for how their answers combine.

Agreed with the custom-providers work
([plan-custom-and-local-providers.md](plan-custom-and-local-providers.md),
P1 = PR #6, P2 next). This builds on its engines, `Capabilities`, `check` and
error `details`, and adds no new extension mechanism.

## The idea in one line

A **panel** is an engine that fans one request out to several models. You
register it under any name and call it like any other model:

```js
const client = new Client(config, {
  engines: { panel: panelEngine({ moderation: { /* see below */ } }) },
});
await client.decide("panel/moderation", request); // same call as "jev/jev-latest"
```

This keeps the library's promise that you switch with one string. Going from
one model to an ensemble is a config change. Your call sites stay the same.

`panel` is a suggested name, not a reserved prefix. That follows the registry
rule D1 (free names, an error on a clash).

## What it reuses

| Need | Provided by |
|---|---|
| Validate the request, check each response | Engine slot (P2): the core runs `validate` before and `check` after |
| Call member models | `opts.decide`, the client's own re-entrant `decide`, passed to engines (P2); `validate`/`check` apply to every member call. `MAX_DEPTH = 4`: deeper nesting fails with `InvalidRequest` (`invalid_config`, field `engines`). Python passes `decide=` only to engines whose signature takes it (sync in `Client`, async in `AsyncClient`) |
| Skip members that can't take the request | `client.capabilities(model)` (async in JS, sync in Python; custom providers return `{}` when undeclared) + core `validate(request, caps)` |
| A key per provider | `byProvider` in client config (P2) |
| Cascade / escalation | `escalate({ to, when })` middleware (P5). Not a panel mode |
| Latency hedging, shadow, pairwise `compare` | Already on main |

What's new here: the panel engine, `src/merge.rs`, and its tests.

## Panel config

Plain data, so it can come from a JSON file or a feature-flag service. It
never holds keys.

```json
{
  "moderation": {
    "mode": "vote",
    "members": [
      { "model": "jev/jev-latest" },
      { "model": "gemini/gemini-decision-1", "weight": 2 },
      { "model": "openai/gpt-6-luna", "enabled": false }
    ],
    "rules": { "quorum": 2, "timeout_ms": 3000 }
  }
}
```

- **On/off:** set `enabled: false` on a member. A panel with one enabled member
  behaves exactly like calling that model directly.
- **Live toggles:** `panelEngine(cfg)` takes the object or a getter
  `() => cfg`, and reads it on every call.
- **Per member:** `weight`, `timeout_ms`.

## Modes

| Mode | Behaviour | Capabilities of the panel |
|---|---|---|
| `vote` (default) | Waits for members, merges answers per question | Intersection of the enabled members' capabilities |
| `all` | Returns every member's raw response; the answer comes from the first member | Intersection |
| `race` | First success wins; the others are aborted | Union, minus members that can't take the request |

## Rules (a fixed set, not a DSL)

| Rule | Meaning | Default |
|---|---|---|
| `quorum` | Minimum members that must answer, else the last member's error is thrown (same code, status and retry hint, reason `below_quorum`) | `1` |
| `timeout_ms` | Panel deadline (host side); merge whatever has arrived | none |
| `merge` | `vote` or `mean` (below) | `vote` |

Disagreement is always reported in `disagreed`, never acted on. An app that
wants to reject a split panel checks `res.disagreed.length`. A "use the first
member" option was dropped: it would put one provider's raw numbers into a
response labelled as vote shares.

Members skipped by capabilities are recorded in `res.panel` and don't count
toward quorum.

## Merging (`src/merge.rs`, pure)

Built. `merge(request, members, rules)` in Rust; `json::merge(request, members, rules)`
for bindings (WASM, Python, UniFFI), where each member is
`{ model, weight?, response | error | skipped: true }`. A member's error
message from the host is dropped (it may come from user code); its code,
status, retry hint and details are kept.


- Runs `DecisionResponse::check` on every member first, even if the app ever
  opts out of checking the final response.
- **Default vote, scale-free.** Yes/no: weighted majority of `p >= 0.5`.
  Choice and score: weighted majority of the top option or level id. Ids are
  the caller's, so they compare across providers. Score `value` is never
  averaged: the merged `value` is the winning level's 0-based index (ADR 0004).
  Merged probabilities are weighted vote shares, so the response's
  `probability_source` is the new value `vote`. This is valid across any source.
- **`merge: "mean"` is opt-in.** If members have different `probability_source`
  values (`provider`, `logprob`, `nli`), it fails with `Unsupported`, reason
  `mixed_probability_source`. It refuses rather than warns, because averaging
  numbers on different scales is the silent degradation ADR 0005 forbids.
  The merged score `value` is the expected index of the mean distribution.
- `Refused` answers abstain, and a question where every member refused stays
  `Refused`. Ties go to the label voted for first, so config order breaks them.
- A member whose response fails `check` counts as failed (`BadResponse`, with
  its prefix as `provider`).
- `input_tokens` is summed over answering members.
- Errors use `Error::detailed`: `provider` = member prefix, `question` = id.
  New `Reason` variants: `below_quorum`, `mixed_probability_source`.

## Output stays a `DecisionResponse`

```js
res.answers.flagged          // merged answer, same shape as one model
res.agreement.flagged        // 0..1, weighted share that agreed with the winner
res.disagreed                // questions where voters weren't unanimous
res.panel["jev/jev-latest"]  // raw response, { error }, or { skipped: reason }, plus ms
```

Middleware composes as before: `compose(client, retry())` retries the panel
as a whole, and retries inside a member come from the client the engine calls.

## Keys

Built in P2, host side only. ADR 0006 still holds: the library never reads env
vars, and callers load keys themselves.

```js
new Client({
  byProvider: {
    jev: { apiKey: process.env.JEV_API_KEY },
    gemini: { apiKey: () => vault.get("gemini") }, // resolved per call
    acme: { headers: [["x-tenant", "t1"]], options: { region: "eu" } },
  },
});
```

- Fields: `apiKey`, `accessToken`, `baseUrl`, `accountId`, `headers`,
  `options`. Each is merged over the top-level fields **for that provider
  only**, and a field set in the entry wins. Any value may be a sync or async
  function, resolved per call. Custom codecs receive their merged slice as `config`.
- Python: `by_provider={"jev": {"api_key": ...}}`. Unknown fields raise
  `InvalidRequest` (`invalid_config`, field `by_provider`).
- **A key only goes to its own provider.** A panel with members from more than
  one provider and only top-level credentials fails with `InvalidRequest`
  (reason `missing_config`, field `byProvider`), so a Jev key is never sent
  to Google.

## Plan

1. **P2 (custom-providers session), built, not yet pushed:** the `{ signal, decide }` engine option
   with a depth guard, `client.capabilities(model)` and `byProvider`.
2. ✅ **Core `merge.rs`** with the new reasons, `json::merge` in every binding,
   tests in `tests/merge.rs` (unanimous, weighted split, ties, refusals,
   `mean` and mixed sources, below quorum, malformed member, bad rules, JSON
   entry and host messages never echoed).
3. **`panelEngine` in JS:** the three modes, enabled flags, getter config,
   abort of losers, the capability skip.
4. **Python parity** (`AsyncClient`).

Rough size: core about 120 lines, JS about 80, plus docs and tests.

## Deliberately not doing

- `cascade` as a panel mode. `escalate` covers it.
- A general rule engine, or learned or auto-tuned weights (calibration
  belongs to M5 and must never be silent).
- Concurrency inside the Rust core. It stays sans-IO.
- A built-in or reserved `panel` prefix.

## Open questions

- Per-question routing (send question X only to Gemini): useful, or premature?
- Should `all` mode exist, or is `vote` plus `res.panel` enough?
