# Custom providers

Your decision API isn't supported? Plug it in and keep everything else: the
canonical request and response, error codes, middleware, and switching by
model string.

Pick the lightest option that works:

| You have | Use | Code |
|---|---|---|
| A server that speaks System One exactly | `systemone/<model>` + `baseUrl` | none |
| The same, behind a gateway or with other auth | add `headers` | none |
| Any HTTP decision API | a **codec**: `build` + `parse` | two functions |
| Something that isn't one HTTP call (local model, rules, a mock) | an **engine**: `decide` | one function |

Whatever you pick, the client:

1. validates the request and checks it against your `capabilities` before your
   code runs (`InvalidRequest` / `Unsupported`, with `details`);
2. checks your answer after (`BadResponse` if a question is missing, a kind or
   id is wrong, or a probability is outside `[0, 1]`);
3. turns anything your code throws, other than `LetsDecideError`, into
   `ProviderError` (see [Errors](#errors));
4. works with every middleware (`retry`, `fallback`, `hedge`, `shadow`, ...).

## Codecs

A codec maps the canonical request to your API's HTTP request, and its reply
back. It does no I/O: the client sends the request with its own `fetch` /
`httpx`, so proxies, timeouts and cancellation work as usual.

### JavaScript

```js
import { Client, classifyHttp } from "letsdecide";

const acme = {
  // Optional. Checked before `build`; missing means "validate only".
  capabilities: { max_questions: 50, media: ["image/png", "image/jpeg"] },

  build(model, request, config) {
    // config: { apiKey, accessToken, baseUrl, accountId, headers, options }
    return {
      url: `${config.baseUrl ?? "https://api.acme.ai"}/v1/decide`,
      headers: { authorization: `Bearer ${config.apiKey}`, "content-type": "application/json" },
      body: JSON.stringify({ model, text: request.input, questions: toAcme(request.questions) }),
    };
  },

  async parse(request, response) {
    // response: { status, headers: [[k, v]], body: string }
    if (response.status >= 300) throw await classifyHttp(response); // same codes as built-ins
    const body = JSON.parse(response.body);
    return { model: body.model, answers: fromAcme(body, request), input_tokens: body.tokens ?? null };
  },
};

const client = new Client({ apiKey: process.env.ACME_KEY }, { providers: { acme } });
await client.decide("acme/acme-decide-2", request);
```

`config.headers` are appended after your codec's headers and replace
same-named ones, as for built-ins. `config.options` is yours: put region,
deployment or anything else your API needs there.

### Python

```python
from letsdecide import Client, classify_http

class Acme:
    capabilities = {"max_questions": 50}

    def build(self, model, request, config):
        # config: {api_key, access_token, base_url, account_id, headers, options}
        return {
            "url": "https://api.acme.ai/v1/decide",
            "headers": {"authorization": f"Bearer {config['api_key']}"},
            "body": json.dumps({"model": model, "text": request["input"]}),
        }

    def parse(self, request, response):
        if response["status"] >= 300:
            raise classify_http(response)
        body = json.loads(response["body"])
        return {"model": body["model"], "answers": ..., "input_tokens": None}

client = Client(api_key="...", providers={"acme": Acme()})
```

`AsyncClient` takes the same codec; `build` and `parse` may also be `async`.

### Rust

Implement `Provider` (`capabilities`, `build`, `parse`) and register it:

```rust
let mut providers = Registry::builtin();
providers.register("acme", Acme)?;
let http = providers.build("acme/acme-decide-2", &req, &cfg)?;
let out = providers.parse("acme/acme-decide-2", &req, &res)?;
```

Use `http_error(res)` for non-2xx replies and `apply_headers(&mut http, cfg)`
last in `build`.

## Engines

An engine answers the request itself. Use one for local models, rules, test
doubles, or providers that need several calls.

```js
const local = {
  capabilities: { question_kinds: ["yesno", "choice"], locality: "device", probabilities: "logprob" },
  async decide(model, request, { signal }) {
    const answers = await myRuntime.score(model, request, signal);
    return { model, answers, input_tokens: null, probability_source: "logprob" };
  },
};
const client = new Client({}, { engines: { local } });
```

Python: an object with `decide(model, request)`. `Client` needs a sync
`decide`; `AsyncClient` takes either.

Set `probability_source` honestly: `provider` (a decision API's numbers),
`logprob` (label log-probs) or `nli` (entailment scores). It defaults to
`provider`.

### The mock engine

For tests: no network, no keys, deterministic.

```js
import { Client, mockEngine } from "letsdecide";
const client = new Client({}, { engines: { mock: mockEngine({ answers: { billing: { kind: "yesno", probability: 0.9 } } }) } });
await client.decide("mock/any", request);
```

Unlisted questions get a neutral answer: yes/no 0.5, the first option (by id),
the middle level, with uniform probabilities. Python: `mock_engine(answers)`.

## Names

Names match `[a-z0-9_-]{1,64}`. A name that's taken (a built-in such as
`openai`, or registered twice) is an `InvalidRequest` with reason
`name_taken`. To override a built-in on purpose, list it in `replace`:

```js
new Client(cfg, { providers: { openai: myOpenAiProxy }, replace: ["openai"] });
```

If a future version adds a built-in with your name, the client will refuse to
start with `name_taken` until you rename or `replace` it. New built-in names
are listed in the changelog. See [ADR 0008](adr/0008-provider-registry-free-names.md).

## Errors

- Throw `LetsDecideError` for failures you can classify. `classifyHttp` /
  `classify_http` gives you the same error a built-in adapter would.
- Anything else you throw becomes `ProviderError` with the provider name and
  the exception **type** only. The original is on `error.original`, kept out of
  `util.inspect`, tracebacks and loggers, because exception messages often hold
  response bodies or keys.
- Don't put request content, bodies or keys in your own error messages either.

## Testing your provider: the conformance kit

Write fixtures in the same format as the core's golden fixtures
(`tests/fixtures/**.json`) and run them against your provider:

```js
import { checkProvider } from "letsdecide/conformance";
const report = await checkProvider(acme, fixtures);
assert.equal(report.failed, 0, JSON.stringify(report.results));
```

```python
from letsdecide.conformance import check_provider
report = check_provider(Acme(), fixtures)
assert report["failed"] == 0, report["results"]
```

A fixture:

```json
{
  "name": "all question kinds",
  "model": "acme/acme-decide-2",
  "config": { "api_key": "k" },
  "request": { "input": "...", "questions": { "...": {} } },
  "http": { "url": "https://api.acme.ai/v1/decide", "headers": [["authorization", "Bearer k"]], "body": {} },
  "response": { "status": 200, "headers": [], "body": {} },
  "expected": { "ok": { "model": "...", "answers": {}, "input_tokens": null } }
}
```

For an error case, `expected` is `{ "error": { "code": "RateLimited", "retry_after_ms": 2000 } }`.
Engines skip `http` and `response`. Header names compare case-insensitively
and in any order; bodies compare as JSON.

Cover all three question kinds, a refusal or error, and your limits. That is
also what a built-in needs, so a provider that passes is ready to upstream:
see [CONTRIBUTING.md](../CONTRIBUTING.md).
