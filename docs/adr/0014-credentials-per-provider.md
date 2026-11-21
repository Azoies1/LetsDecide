# 0014: Credentials and endpoints are set per provider only

- Status: Accepted
- Date: 2026-11-20

## Context
One client often talks to several providers: `fallback`, `hedge`, `shadow`,
panels and engines that call other models. A client-wide `apiKey` was sent to
whichever provider a call went to, so a Jev key could reach Google. A
"first provider claims the key" guard would have fixed the leak but makes
errors depend on call order. The library has no users yet, so there is no
compatibility to keep.

## Decision
Bindings take credentials and endpoints only per provider prefix:
`byProvider` (JS), `by_provider` (Python), `byProvider: [String:
ProviderSettings]` (Swift), `byProvider: Map<String, ProviderSettings>`
(Kotlin). Each entry holds `apiKey`, `accessToken`, `baseUrl`, `accountId`,
`headers` and `options`, and a provider only ever sees its own entry. Values
may be functions, called per request (ADR 0007's refresh model is unchanged).
Passing a top-level `apiKey`, `accessToken`, `baseUrl`, `accountId` or
`options` is an `InvalidRequest` (`invalid_config`, field = the name) at
construction, with a message pointing at the per-provider field; so is an
unknown field in an entry. Client-wide `headers` remain for tracing and
gateways; they go to every provider and must not hold secrets, and a
provider's own headers win on a name clash. The Rust core's `Config` is
already per call, so it is unchanged.

## Consequences
- A key cannot reach another provider, whatever middleware or engine routes
  the call. No order-dependent rules.
- Single-provider setups write one more level of nesting
  (`{ byProvider: { gemini: { apiKey } } }`).
- The conformance kit's fixture `config` is one provider's entry, as before.
