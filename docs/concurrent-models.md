# Concurrent models (panels)

Status: **built** for Rust (`merge`), JS and Python (`panelEngine` /
`panel_engine`). One item is still open: the cross-provider key guard (see
[Keys](#keys)).

Ask several decision models the same request at once, turn each on or off, and
set a few rules for how their answers combine. This builds on the
custom-providers work ([plan-custom-and-local-providers.md](plan-custom-and-local-providers.md),
PRs #6 and #7): engines, `check`, error `details` and `byProvider`. It adds no
new extension mechanism.

## The idea in one line

A **panel** is an engine that fans one request out to several models. You
register it under any name and call it like any other model:

```js
const client = new Client(config, {
  engines: { panel: panelEngine({ moderation: { /* see below */ } }) },
});
await client.decide("panel/moderation", request); // same call as "jev/jev-latest"
```

```python
client = AsyncClient(engines={"panel": panel_engine({"moderation": {...}})})
await client.decide("panel/moderation", request)
```

This keeps the library's promise that you switch with one string. Going from
one model to an ensemble is a config change, and your call sites stay the same.
`panel` is a suggested name, not a reserved prefix, following registry rule D1.

## Panel config

The config is plain data, so it can come from a JSON file or a feature-flag
service. It never holds keys.

```json
{
  "moderation": {
    "mode": "vote",
    "members": [
      { "model": "jev/jev-latest" },
      { "model": "gemini/gemini-decision-1", "weight": 2 },
      { "model": "openai/gpt-6-luna", "enabled": false }
    ],
    "rules": { "quorum": 2, "timeout_ms": 3000, "merge": "vote" }
  }
}
```

- **On/off:** set `enabled: false` on a member, and it is never called. A panel
  with one enabled member gives that member's answer as vote shares.
- **Live toggles:** `panelEngine(cfg)` takes the object or a function
  returning it (sync or async), and reads it on every call.
- **Weight:** per member, default `1`, must be positive.
- **Routing:** `questions: ["nsfw"]` on a member asks it only those questions
  (see [Routing](#routing)). Without it, a member answers everything.

## Routing

Send each question only to the models you trust for it:

```json
{
  "triage": {
    "members": [
      { "model": "gemini/gemini-decision-1", "questions": ["nsfw", "violence"] },
      { "model": "jev/jev-latest", "questions": ["topic"] },
      { "model": "openai/gpt-6-luna" }
    ]
  }
}
```

- Each member gets a request with only its questions, so it is billed for
  less and its own limits (question count, kinds) apply to that subset.
- Each question is voted on only by the members routed to it. `quorum`
  applies **per question**; without routing that is the same as before.
  Below quorum, the error names the `question` and the last failing member.
- **Checked before any call:** a question no member covers (`InvalidRequest`,
  field `members`, with the `question`), an unknown or empty `questions` list
  (field `questions`), and routing in `race` mode (field `questions`: the
  winner must answer everything). `merge` repeats the first two checks for
  direct callers.
- A member that answers questions it wasn't asked fails `check` and counts as
  failed.

## Modes

| Mode | Behaviour |
|---|---|
| `vote` (default) | Calls every enabled member, merges answers per question |
| `race` | Calls every enabled member; the first answer wins and the others are aborted. The winner's response is returned unchanged |

`AsyncClient` (Python) and JS run members concurrently. Python's sync
`Client` runs them in order: in `race` mode it stops at the first answer, and
`timeout_ms` is checked between members.

## Rules (a fixed set, not a DSL)

| Rule | Meaning | Default |
|---|---|---|
| `quorum` | Minimum members that must answer. Below it, the last member's error is thrown (same code, status and retry hint), with reason `below_quorum` and that member as `provider` | `1` |
| `timeout_ms` | Panel deadline. Members still running are aborted and count as `ProviderUnavailable`; whatever has arrived is merged | none |
| `merge` | `vote` or `mean` (below) | `vote` |

- **Members that can't take the request are skipped.** The client validates
  each member call before any network request, so a member over its limits
  (for example Clef with more than 64 questions) throws `Unsupported` at no
  cost. The panel records it as `skipped`, and it doesn't count toward quorum.
- **Disagreement is reported, never acted on.** It appears in `disagreed`. An
  app that wants to reject a split panel checks `res.disagreed.length`.

## Merging (`src/merge.rs`, pure)

`merge(request, members, rules)` in Rust, and `json::merge` for the bindings
(WASM, Python, UniFFI). Each member is
`{ model, weight?, questions?, response | error | skipped: true }`. The panel engines call
it; it is also usable directly.

- Every member's response is run through `DecisionResponse::check` first,
  against the questions it was asked. One that fails counts as failed (`BadResponse`, with its prefix as `provider`).
- **Vote (default), scale-free:**
  - Yes/no: weighted majority of `p >= 0.5`.
  - Choice and score: weighted majority of the top option or level id. The ids
    are the caller's, so they compare across providers.
  - Merged probabilities are vote shares, so `probability_source` is `vote`.
  - Score `value` is the winning level's 0-based index (ADR 0004), never an
    average of the members' values.
  - Valid across any `probability_source`.
- **`mean` (opt-in):** a weighted average of probabilities. If the members'
  `probability_source` values differ, it fails with `Unsupported`, reason
  `mixed_probability_source`. It refuses rather than warns, because averaging
  numbers on different scales is the silent degradation ADR 0005 forbids.
  Score `value` is the expected index of the mean distribution.
- **Abstentions and ties:** `Refused` answers abstain, and a question where
  everyone refused stays `Refused`. Ties go to the label voted for first, so
  config order breaks them.
- **Totals:** `input_tokens` is summed.
- **Errors passed in from host code:** the message is dropped (it may come
  from user code). Code, status, retry hint and details are kept.

## Output

```js
res.answers.flagged          // merged answer, same shape as a single model's
res.agreement.flagged        // vote: 0..1, weighted share that agreed with the result
res.disagreed                // vote: questions where voters weren't unanimous
res.panel["jev/jev-latest"]  // { response } | { error: { code, details } } | { skipped: true }, plus ms
```

A panel error carries the same `panel` report (`err.panel`).

The response keeps its normal shape, so middleware composes as usual. To keep
the extra fields, the client now returns
`{ ...engineOutput, ...checkedResponse }` instead of only the checked response.
That is a small change to the P2 client, discussed with its author.

Nesting is allowed up to `MAX_DEPTH` (4). A panel that lists itself fails with
`InvalidRequest`.

## Keys

`byProvider` (P2) gives each provider its own credentials. ADR 0006 holds: the
library never reads env vars, and callers load keys themselves.

```js
new Client({
  byProvider: {
    jev: { apiKey: process.env.JEV_API_KEY },
    gemini: { apiKey: () => vault.get("gemini") }, // resolved per call
  },
});
```

**Open, awaiting the maintainer's decision: a guard so a key only goes to its own provider.** It would change behaviour for clients that use one top-level key for two providers today, so it needs a CHANGELOG "Changed" entry and an ADR extending 0006. Engines never see the
config, so the guard can't live in the panel. `fallback` has the same leak
today. The proposal, under discussion with the custom-providers work, puts it
in the client: remember the first provider that received top-level
credentials, and refuse (`InvalidRequest`, reason `missing_config`, field
`byProvider`) to send them to a different provider that has no `byProvider`
entry. Until it lands, set `byProvider` whenever a panel spans providers.

## Deliberately not doing

- `cascade` as a mode: the planned `escalate({ to, when })` middleware covers it.
- `all` as a mode: `vote` plus `res.panel` already returns every raw response
  (decided by the maintainer).
- Per-member timeouts, a rule language, and learned weights (calibration
  belongs to M5 and must never be silent).
- Concurrency inside the Rust core. It stays sans-IO.
