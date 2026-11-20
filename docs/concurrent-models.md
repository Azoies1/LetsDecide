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
| Call member models | `opts.decide`, the client's own re-entrant `decide`, passed to engines (P2), with a nesting-depth guard so a panel can't call itself forever |
| Skip members that can't take the request | `client.capabilities(model)` + core `validate(request, caps)` |
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
    "rules": { "quorum": 2, "timeout_ms": 3000, "on_disagree": "flag" }
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
| `quorum` | Minimum successful members, else throw the last error | `1` |
| `timeout_ms` | Panel deadline; merge whatever has arrived | none |
| `on_disagree` | `flag` (marked in the output) · `error` · `primary` (use the first member) | `flag` |

Members skipped by capabilities are recorded in `res.panel` and don't count
toward quorum.

## Merging (`src/merge.rs`, pure)

`merge(request, [(member, response | error, weight)], rules)`:

- Runs `DecisionResponse::check` on every member first, even if the app ever
  opts out of checking the final response.
- **Default vote, scale-free.** Yes/no: weighted majority of `p >= 0.5`.
  Choice and score: weighted majority of the top option or level id. Ids are
  the caller's, so they compare across providers. Score `value` is never
  averaged. This is valid across any `probability_source`.
- **`merge: "mean"` is opt-in.** If members have different `probability_source`
  values (`provider`, `logprob`, `nli`), it fails with `Unsupported`, reason
  `mixed_probability_source`. It refuses rather than warns, because averaging
  numbers on different scales is the silent degradation ADR 0005 forbids.
- `Refused` answers abstain, and a question where every member refused stays
  `Refused`.
- Errors use `Error::detailed`: `provider` = member prefix, `question` = id.
  New `Reason` variants: `mixed_probability_source`, `below_quorum`,
  `members_disagree`.

## Output stays a `DecisionResponse`

```js
res.answers.flagged          // merged answer, same shape as one model
res.agreement.flagged        // 0..1, weighted share that agreed with the winner
res.disagreed                // ["flagged"] when on_disagree = "flag"
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

- Each entry is merged over the top-level fields **for that provider only**.
  Custom codecs receive their merged slice as `config`.
- **A key only goes to its own provider.** A panel with members from more than
  one provider and only top-level credentials fails with `InvalidRequest`
  (reason `missing_config`, field `byProvider`), so a Jev key is never sent
  to Google.

## Plan

1. **P2 (custom-providers session):** the `{ signal, decide }` engine option
   with a depth guard, `client.capabilities(model)` and `byProvider`.
2. **Core `merge.rs`** with the new reasons. Golden tests: unanimous, split,
   all refused, mixed `probability_source` under `mean`, below quorum, a
   malformed member.
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
