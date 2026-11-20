# Plan: custom providers and local models

Status: **in progress**. P1 and P2 are built; P3 onward are planned. Decisions below
are recorded as ADRs 0008–0013 in [adr/](adr/README.md) and summarised in
[design.md](design.md); open items are in [future-work.md](future-work.md).

## Goal

1. **Bring your own provider.** A user whose decision API we don't support
   (another cloud, an in-house service, a gateway) can plug it in and keep
   everything else: the canonical request and response, the error codes,
   middleware (retry, fallback), and switching by model string.
2. **Local models.** The same request runs on a model on the user's own
   hardware: a server or PC (GPU box, laptop), a phone (iOS, Android), and the
   browser.

Both have to keep the promises in the README: one canonical shape, sans-IO
core, small, and honest (no silent degradation, numbers never rescaled).

## Where we are

- `Provider` (`build` + `parse`) is already a clean extension point, **but only
  in Rust**: `resolve()` is a closed `match`, and `json::build/parse` (which every
  binding uses) call `resolve()`. JS and Python users can't add a provider.
- `systemone/<model>` + `base_url` already covers any server that speaks the
  System One shape exactly. Small differences (auth header, path, `state` vs
  `input`) have no knob.
- `Config` is a fixed struct (`api_key`, `base_url`, `account_id`). Custom
  providers need their own settings (region, deployment, extra headers).
- design.md lists as non-goals *running models locally* and *emulating
  decisions with a chat LLM*. Goal 2 changes the first and needs a careful line
  on the second (see [D2](#d2-local-models-in-scope-parsing-generated-text-stays-out-decided)).
- design.md defers a capability struct "until a second adapter needs shared
  validation". Custom providers plus local-first routing are that need.

## The extension ladder

Users climb only as far as they need. Each rung is cheaper to maintain than
the next.

| Rung | What the user writes | Covers | Sans-IO? |
|---|---|---|---|
| **0. Config** | `base_url`, `headers` | Compatible server, gateway, proxy, regional endpoint | Yes |
| **1. Dialect** | Data: tweaks to a known wire family | "Speaks System One / OpenAI, except…" | Yes |
| **2. Codec** | Two functions, `build` + `parse`, in their language | Any HTTP decision API | Yes |
| **3. Engine** | One async function, `decide(request)` | Anything not over HTTP: in-process local models, rules, mocks, multi-call flows | No (lives in the host) |

Every rung gets the same guarantees from the core:

- the request is validated **before** the provider sees it
  (`DecisionRequest::validate()` plus the provider's declared capabilities);
- the response is checked **after** (new: `DecisionResponse::check(&req)`:
  every question answered, answer kind matches question kind, ids are the
  caller's, probabilities finite and in `[0, 1]`). A buggy custom provider
  fails with `BadResponse`, it can't leak a malformed answer into the app;
- HTTP errors are classified the same way (`http_error`, exposed to bindings);
- middleware (retry, fallback, future hedging/shadow) works unchanged because
  it wraps `decide`, not the provider.

### Rung 0: config

Add to `Config`:

```rust
pub headers: Vec<(String, String)>,      // added after the adapter's own; redacted in Debug
pub options: serde_json::Map<String, Value>, // free-form, read by custom providers
```

`headers` alone covers API gateways (AI Gateway, Portkey, Kong), Azure-style
`api-key` headers, tenant headers and tracing headers. `options` is how rungs
1–3 get their own settings without growing `Config` per provider.

### Rung 1: dialects

The two wire families are already parameterised internally (`Preset`). Expose
the knobs that actually varied between Jev, Clef and Gemini, as data:

```json
{
  "family": "systemone",
  "url": "https://decide.acme.internal/v2/decide",
  "auth": { "header": "x-api-key", "scheme": null },
  "input_field": "input",
  "model_in": "body",
  "limits": { "questions": 100, "options": 50, "levels": 10 },
  "media": ["image/png", "image/jpeg"]
}
```

plus an `openai` family with `url` and `auth` (OpenAI-compatible endpoints,
Azure, proxies). Deliberately **not** a general templating DSL: if a vendor
needs more than these knobs, that's a codec (rung 2). Dialects are data, so
they are safe to load from config files and per-tenant settings (no code
execution) — with one caveat: a tenant-controlled `url` is an SSRF vector, so
the docs must say to allowlist hosts server-side.

### Rung 2: codecs in the host language

A codec is the `Provider` trait in the user's language. The core keeps doing
validation and checking around it.

Rust (no globals, consistent with the core's rules: a registry is a value):

```rust
let mut providers = Registry::builtin();          // jev, clef, gemini, openai, systemone
providers.register("acme", Box::new(AcmeProvider))?; // error if "acme" is taken
let (p, model) = providers.resolve("acme/acme-decide-2")?;
```

`resolve()` stays as a shortcut for `Registry::builtin().resolve()`. Registering
a taken prefix fails unless the caller says `replace` ([D1](#d1-custom-prefixes-free-names-error-on-clash-decided)), so nobody shadows
`openai/` by accident.

JavaScript:

```js
const client = new Client(config, {
  providers: {
    acme: {
      build(model, request, config) {          // pure
        return { method: "POST", url: "https://api.acme.ai/decide",
                 headers: [["authorization", `Bearer ${config.apiKey}`]],
                 body: JSON.stringify(toAcme(model, request)) };
      },
      parse(request, response) {               // pure; may throw LetsDecideError
        if (response.status >= 400) throw classifyHttp(response); // core's classification
        return fromAcme(JSON.parse(response.body), request);
      },
      capabilities: { maxQuestions: 50, media: ["image/png"] },
    },
  },
});
await client.decide("acme/acme-decide-2", request);
```

Python mirrors it with a `Protocol` (`build`, `parse`, optional
`capabilities`). Swift/Kotlin get the same once those bindings exist.

New core JSON entry points, so host codecs reuse the core instead of
re-implementing it: `validate(request, capabilities)`, `check(request,
response)`, `classify_http(response)`.

### Rung 3: engines

For anything that isn't one HTTP round trip:

```ts
interface Engine {
  decide(model: string, request: DecisionRequest, opts: { signal?: AbortSignal }):
    Promise<DecisionResponse>;
  capabilities?: Capabilities;
}
```

Registered the same way (`engines: { local: myEngine }`). The client still
validates the request and checks the response. Engines are where local models
live (next section), and also mocks, rules, and providers that need several
calls.

### Capabilities

The deferred capability struct, now needed by rungs 1–3 and by routing:

```rust
pub struct Capabilities {
    pub max_questions: Option<usize>,
    pub max_options: Option<usize>,
    pub max_levels: Option<usize>,
    pub media: Vec<String>,            // accepted MIME types or "image/*"
    pub probabilities: ProbSource,     // see D3
    pub locality: Locality,            // Cloud | Network (your server) | Device
}
```

Built-ins return theirs (moving today's `Limits` into it). `fallback` can then
skip a model that can't take the request **before** calling it, and a new
`route` middleware can pick by locality.

## Local models

### What a local "decision" is

Cloud decision APIs return probabilities per option. A local model can give
real probabilities in two principled ways; anything else is a guess.

1. **Label scoring with log-probs** (any causal LM, incl. small ones):
   prompt with the instructions, the input and labelled options, then read the
   next-token distribution over the label tokens only.
   - yes/no: P(" Yes") vs P(" No"), renormalised over the two.
   - choice: options labelled `A`, `B`, …; probabilities renormalised over the
     labels; `choice` = argmax; `confidence` = `None` (no separate signal).
   - score: levels labelled `0`…`n-1`; `value` = probability-weighted index.
     **This is the same 0-based expected-index semantics as Jev/Clef/Gemini**,
     so scores stay comparable in kind across providers.
   - Option count is bounded by single-token labels (about 26); more is
     `Unsupported`. Position bias is reduced by an opt-in `permutations: k`
     (average over k shuffles, k× cost).
   - No free text is generated or parsed.
2. **Zero-shot NLI classifiers** (DeBERTa-MNLI class, tens of MB, run
   anywhere ONNX runs): entailment probability for "this text: <option>".
   Good fit for yes/no and choice on phones; weak for score.

Runtimes that don't expose log-probs (for example, as far as I know, Apple's
on-device Foundation Models and Android's Gemini Nano APIs; verify before
building) could only produce sampled answers, so they return `Unsupported` ([D3](#d3-runtimes-without-log-probs-return-unsupported-with-details-decided)).

### Keep the algorithm in the core (sans-IO scorer)

The scoring logic (prompt template, label assignment, renormalisation,
expected index, permutation averaging) is the hard, testable part. Put it in
Rust once, sans-IO like everything else:

```
core.score_plan(request)       -> [{ question_id, prompt, candidates: ["A","B","C"] }]
host runtime                   -> log-probs for each candidate after each prompt
core.score_answers(request, logprobs) -> DecisionResponse
```

Then every platform only writes a tiny runtime shim ("given this prompt, give
me log-probs for these candidate tokens"), and every platform gets identical
answers for identical log-probs. Golden fixtures test it offline like the
adapters. Prompts per question share a prefix (instructions differ, input does
not), so runtimes with prefix caching pay for the input once.

### Runtimes per platform

| Platform | Runtime options for the shim | Packaging |
|---|---|---|
| Server / PC | llama.cpp server (`n_probs`), vLLM / SGLang (OpenAI-compatible `logprobs`), Ollama (where its log-prob support is enough), llama-cpp-python, HF transformers | Python & JS engines; **plus a sidecar** (below) |
| Browser / Electron | transformers.js (WebGPU/WASM), WebLLM | JS engine; model download is the user's job |
| iOS / macOS | llama.cpp, MLX, Core ML (NLI) | Swift binding + engine |
| Android | llama.cpp, ONNX Runtime Mobile (NLI), ExecuTorch, MediaPipe | Kotlin binding + engine |
| Edge / IoT (Pi, Jetson) | llama.cpp, ONNX Runtime | Sidecar or Python engine |

### The sidecar: `letsdecide-serve`

A small server that speaks **System One** (`POST /v1/systemone`) and answers
with the scorer over a local runtime. Then:

```js
client.decide("systemone/qwen3-1.7b", req) // byProvider.systemone.baseUrl: "http://localhost:8787"
```

works from **every** binding with zero new client code, including from
languages we have no binding for, and lets one GPU box serve a whole team.
This is the fastest way to deliver goal 2 for servers and PCs. In-process
engines are still needed for phones and browsers, where a server isn't an
option.

## Routing: local-first, cloud when needed

The scenarios below lean on three middleware additions (JS first, then
Python), all plain `(next) => decide` wrappers:

- **`escalate({ to, when })`**: answer locally; if `when(response)` is true
  (e.g. any yes/no probability between 0.2 and 0.8, or a refusal), re-ask the
  cloud model. Response records `escalatedFrom`.
- **capability-aware `fallback`**: skip models whose capabilities can't take
  the request (e.g. audio to a text-only local model) instead of calling and
  catching `Unsupported`.
- **`shadow({ model, sample, onResult })`**: also send a sample to a second
  model, never block on it; feeds M5 (evaluation) and the
  "should we move to local?" question.

## Usage scenarios

Grouped by who is asking. Each names the rung or feature it needs, which is
how the phases below were ordered.

### Custom cloud and in-house

1. **Vendor we don't support yet.** "Acme Decide" launches; a team writes a
   codec in TypeScript in an afternoon, ships it as `letsdecide-provider-acme`,
   and later upstreams it with fixtures. → rung 2, conformance kit.
2. **In-house decision service** that copied Jev's API but uses `x-api-key`
   and `/v2/decide`. → rung 1.
3. **Data residency / sovereign cloud**: same vendor, EU or gov endpoint. →
   rung 0 (`base_url`).
4. **API gateway in front of every provider** for spend control and logging,
   needing extra headers. → rung 0 (`headers`).
5. **Classic ML endpoint** (SageMaker, Vertex, a FastAPI scikit-learn model)
   returning class probabilities, exposed as a `choice` question so product
   code doesn't care whether it's an LLM. → rung 2.
6. **Multi-tenant SaaS, bring your own model**: each customer stores a
   dialect in settings; the app never runs customer code. → rung 1 (data
   only), host allowlist for SSRF.
7. **Rules or keyword pre-filter as a provider** ("obvious spam" regex
   answers with probability 1.0; everything else falls through). → rung 3 +
   `fallback`.
8. **Tests without network or keys**: a `mock/` engine that returns fixed or
   seeded answers, and a record/replay engine built on the fixture format. →
   rung 3; ship `mock` built in.

### Local: server and PC

9. **High-volume batch triage** (millions of tickets a night) on a rented GPU
   with vLLM; cloud only for the uncertain tail. → sidecar + `escalate`.
10. **Air-gapped or regulated sites** (hospital, defence, bank) that can't
    send data out at all. → sidecar, `locality: Device/Network` enforced by a
    `route` policy that refuses cloud models.
11. **Desktop app** (Electron/Tauri) sorting the user's email or files
    privately on their laptop. → JS engine over llama.cpp, or sidecar bundled
    with the app.
12. **CI and local dev** run decisions against a small local model so tests
    don't need keys or cost money; nightly job compares with the cloud. →
    sidecar + `shadow`.

### Local: mobile and browser

13. **Private journaling / health app** flags crisis language on-device;
    the text never leaves the phone. → Swift/Kotlin engine (NLI or small LM).
14. **Offline field work**: an inspector photographs a part, "is there
    corrosion? yes/no", no signal on site. → mobile engine with a small vision
    model; image capability declared.
15. **Browser extension** classifying the page being read without shipping a
    key to the browser (fixes the "a key in client code is public" problem in
    design.md). → JS engine on WebGPU.
16. **Content moderation before upload**: on-device check first, cloud only
    if borderline. → engine + `escalate`.
17. **Smart home / IoT** ("is this a delivery or a stranger?") on a Pi or
    Jetson. → sidecar.

### Lifecycle

18. **Migrate cloud → local safely**: run local in `shadow` for a week,
    compare agreement per question, then flip the model string. → `shadow`,
    M5.
19. **Distil**: log cloud decisions, fine-tune a small local model on them,
    serve it via the sidecar, switch with one string. The library doesn't
    train; it makes the swap free.
20. **Cost ceiling**: local by default, cloud only when the local answer is
    uncertain and the monthly budget allows. → `escalate` with a `when` that
    checks a budget.
21. **Resilience**: cloud is down or rate-limited; fall back to the local
    model with a flag in the response so the app can re-check later. →
    `fallback` (exists) + engine.

## Phases

Each phase is shippable on its own and follows CONTRIBUTING (smallest useful
change, fixtures, no silent degradation).

| # | Deliverable | Done when |
|---|---|---|
| **P1** ✅ | Core: `Registry`, `Capabilities` (built-ins migrated from `Limits`), `DecisionResponse::check`, `Config.headers/options`, JSON entry points `validate`/`check`/`classify_http` | Built-ins pass unchanged fixtures; a Rust test registers a custom provider; `check` rejects every malformed-response fixture |
| **P2** ✅ | Host providers: JS and Python accept `providers` (codecs) and `engines`; built-in `mock/` engine; **conformance kit** (`checkProvider(provider, fixtures)`) reusing the golden fixture format; guide `docs/custom-providers.md` | A sample `acme` codec passes the kit in JS and Python; middleware works over it |
| **P3** | Dialects for `systemone` and `openai` families | Jev, Clef, Gemini presets re-expressed as dialects produce byte-identical fixtures |
| **P4** | Local scorer in core (`score_plan`/`score_answers`), golden fixtures from recorded log-probs; `letsdecide-serve` sidecar over llama.cpp server and an OpenAI-compatible (vLLM) backend | `systemone/<local-model>` works from JS and Python against the sidecar; scorer fixtures cover all three question kinds and the label limit |
| **P5** | In-process engines: Python (llama-cpp-python), JS (transformers.js); `escalate` and capability-aware `fallback` middleware (`shadow` already ships) | Scenario 16 (local, escalate to cloud) runs end-to-end in an example |
| **P6** | Engine slots, `headers`/`options` and error `details` in the Swift and Kotlin clients (the UniFFI bindings already ship); reference engines for llama.cpp (iOS/Android) and ONNX NLI | Example iOS and Android apps answer a yes/no offline |

P1 is small and unblocks everything else. P4 delivers server/PC local support
without any mobile work, so it can ship before P6.

## Decisions

### D1. Custom prefixes: free names, error on clash (decided)

`register("acme", …)` fails with `InvalidRequest` if the name is a built-in
or already registered, unless the caller passes `replace`. Same check in every
binding.

The alternative was a reserved namespace (`x-acme/`). Its one benefit: we
promise never to ship a built-in starting with `x-`, so a custom name can
never clash with a provider we add later. With free names, if we later add a
built-in `acme`, the user's `register("acme")` starts failing after an
upgrade. That failure is loud, happens at start-up (registration, not at
request time), and the message names the clash, so the cost is small. Free
names keep model strings readable (`acme/model`, not `x-acme/model`), which
matters because the whole pitch is "change one string". The changelog lists
every new built-in prefix.

### D2. Local models in scope; parsing generated text stays out (decided)

design.md's non-goals change to: *running models locally* is a goal (scorer +
engines); *getting decisions by parsing a model's generated text* stays a
non-goal. Reasons:

- **The numbers would be invented.** If a model writes "probability: 0.8",
  that 0.8 is text the model produced, not a measured probability. It is
  usually poorly calibrated, clusters on round numbers, and changes with
  phrasing. The README promises probabilities are the provider's real numbers;
  a generated number would break that promise quietly.
- **It breaks in ways the core can't classify.** Free text drifts from the
  format ("Yes, mostly", extra prose, wrong label, half a JSON object). Every
  failure would need guesswork to map onto our answer shape, which is the
  "silent degradation" CONTRIBUTING forbids.
- **It's not repeatable.** Sampling makes the same request give different
  answers; label scoring is one deterministic forward pass.
- **It's slower and costs more.** Generation runs many decoding steps; scoring
  reads one next-token distribution per question, and the input is shared
  across questions.

Label scoring reads the model's own probabilities directly, so it is in scope.

### D3. Runtimes without log-probs return `Unsupported`, with details (decided)

No sampling mode for now. Instead, `Unsupported` (and other errors) gain a
structured, safe `details` field so users can see exactly why:

```json
{
  "code": "Unsupported",
  "message": "local engine cannot score label `C` for question `topic`: it spans 2 tokens",
  "details": {
    "provider": "local",
    "reason": "multi_token_label",
    "question": "topic",
    "limit": 1,
    "actual": 2
  }
}
```

`reason` is a stable enum: `no_logprobs`, `multi_token_label`,
`too_many_options`, `too_many_levels`, `too_many_questions`,
`media_not_accepted` (with the MIME type), `question_kind` (e.g. NLI engine
and `score`), `missing_config` (field name). Built-in adapters move their
existing limit errors onto the same reasons, and every response also gets
`probability_source`: `provider | logprob | nli` (design.md's planned
`confidence_source`).

**What `details` may contain:** provider name, model name, reason, the caller's
question/option/level ids (always `[a-z0-9_]`), counts and limits, MIME types,
config **field names**, HTTP status, our own label tokens (`A`, `Yes`).

**What it must never contain** (same rule as today, now enforced by tests):
API keys or any header value, `input`, media bytes, instructions, option or
level descriptions, request or response bodies, URL query strings, and config
values (only names). A test builds every error with a canary string in each
of these places and fails if the canary appears in the serialised error or
its `Display`.

Custom codecs and engines are user code, so the core can't vet their
messages. The bindings wrap anything they throw: our own `LetsDecideError`
passes through; any other exception becomes `ProviderError` with the
provider name and the exception **type** only, and the original is kept on
`cause` in memory for the user's own debugging, never serialised by us. The
guide tells codec authors not to put bodies in messages.

### D4. Default local model: the consumer's choice (decided)

The library names no default model and fixtures use recorded log-probs, not a
specific model. Docs show how to plug in a model, not which one. Choosing a
model with the library's help is future work (see
[future-work.md](future-work.md#model-selection)).

### D5. Packaging (proposed)

Rule: **if it pulls in a model runtime, it's a separate package; if it only
needs HTTP or our own code, it stays in the main package.** The main packages
stay as small as today.

| Piece | Where | Why |
|---|---|---|
| Engine and codec slots, registry, `check`, `mock` engine | main packages | no deps |
| Scorer (`score_plan`/`score_answers`) | Rust core, so every binding has it | pure Rust, a few KB |
| Middleware (`escalate`, `shadow`, capability `fallback`) | main JS / Python packages | no deps |
| HTTP runtime shims: llama.cpp server, vLLM/SGLang (OpenAI-compatible log-probs), Ollama | main packages | only `fetch`/`httpx`, already used |
| `letsdecide-serve` sidecar | separate binary and container image | it's a server, not a library |
| Python in-process: llama-cpp-python, transformers, ONNX Runtime | `letsdecide[llamacpp]`, `[transformers]`, `[onnx]` extras, lazy-imported | same pattern as today's `[httpx]` |
| JS in-process: transformers.js, WebLLM | `@letsdecide/transformers`, `@letsdecide/webllm` | multi-MB deps, browser-only parts |
| Swift: llama.cpp, MLX, Core ML | separate SwiftPM products in the Swift package | apps link only what they use |
| Kotlin: llama.cpp, ONNX Runtime Mobile, ExecuTorch | separate Gradle artifacts | same, plus native libs per ABI |

### D6. Support tiers in the README (proposed)

*Built-in* (verified, fixtures in this repo), *Community* (passes the
conformance kit, listed, not maintained by us), *Custom* (yours).

## Risks

- **Scorer quality varies a lot** by model and prompt. Mitigation: never
  claim calibration; expose `probability_source`; ship `shadow` so users
  measure before trusting.
- **Tokeniser edge cases** (labels split into several tokens, leading spaces).
  Mitigation: the runtime shim reports the token ids it scored; the scorer
  rejects multi-token labels with `Unsupported` rather than guessing.
- **Binary size on mobile.** The core stays tiny; model runtimes are the
  user's choice and live in separate packages ([D5](#d5-packaging-proposed)).
- **Custom code is outside our control.** `check()` stops malformed answers;
  it can't stop wrong ones. The conformance kit and docs make that boundary
  explicit.
- **SSRF via dialect URLs** in multi-tenant setups. Document an allowlist.
