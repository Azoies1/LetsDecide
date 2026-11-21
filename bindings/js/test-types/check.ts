// Type-checks the package the way a Node ESM (NodeNext) consumer sees it. Relative
// specifiers inside the .d.ts files need explicit `.js` extensions, or the
// middleware exports silently vanish for those users.
import { Client, LetsDecideError, compose, fallback, retry } from "../index.js";
import { checkProvider } from "../conformance.js";
import type { DecisionRequest } from "../index.js";

const client = new Client({ byProvider: { gemini: { apiKey: "k" } } });
const decide = compose(client, fallback(["jev/jev-latest"]), retry());
const request: DecisionRequest = {
  input: "x",
  questions: { q: { kind: "yesno", instructions: "?" } },
};

export async function run() {
  const response = await decide("gemini/gemini-decision-1", request);
  return response.answers.q;
}
export { LetsDecideError, checkProvider };
