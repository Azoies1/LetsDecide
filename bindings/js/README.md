# letsdecide (JavaScript / TypeScript)

One request shape for every decision API (Jev, Clef, OpenAI, Gemini). The core
is Rust compiled to WebAssembly (146 KB gzipped); it builds the HTTP request
and parses the reply, and `fetch` sends it. You call providers directly with
your own keys. There is no gateway.

Works in Node, browsers and edge runtimes. Includes TypeScript types.

## Install

```sh
npm install letsdecide
```

## Usage

```js
import { Client } from "letsdecide";

const client = new Client({ apiKey: process.env.GEMINI_API_KEY });

const result = await client.decide("gemini/gemini-decision-1", {
  input: "I was charged twice.",
  questions: {
    billing: { kind: "yesno", instructions: "Is this about billing?" },
  },
});

result.answers.billing; // { kind: "yesno", probability: 0.97 }
```

Change the model string (`"provider/model"`) to change provider. Supported:
`jev/<model>`, `clef/<model>`, `gemini/<model>`, `openai/<model>` and
`systemone/<model>` for any System One compatible server.

### Node

Node's `fetch` can't load the `.wasm` file from a `file:` URL, so pass the
bytes:

```js
import { readFileSync } from "node:fs";
import { Client } from "letsdecide";

const wasm = readFileSync(
  new URL("./pkg/letsdecide_wasm_bg.wasm", import.meta.resolve("letsdecide")),
);
const client = new Client({ apiKey: "..." }, { wasm });
```

### Errors

Failures throw `LetsDecideError` with `code`, `status`, `retryable` and
`retryAfterMs`. Keys and request bodies are never included.

### Retry and fallback

```js
import { Client } from "letsdecide";
import { compose, retry, fallback } from "letsdecide/middleware.js";

const decide = compose(new Client({ apiKey: "..." }), fallback(["jev/jev-latest"]), retry());
const res = await decide("clef/clef-flash", request);
```

## More

Full documentation, the answer and error reference, and the other bindings
(Rust, Python): <https://github.com/azoies1/letsdecide>

Apache-2.0.
