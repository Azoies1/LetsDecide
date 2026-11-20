# 0001: Sans-IO core

- Status: Accepted
- Date: 2026-11-19

## Context
Providers differ in wire format, not in what they decide. Network time
(40 to 250 ms) dwarfs translation time (microseconds). Callers have strong
opinions about HTTP stacks, proxies, pinning and cancellation, and the library
must also compile to WebAssembly and bind to other languages.

## Decision
The core only has two pure functions: `build` (request to `HttpRequest`) and
`parse` (`HttpResponse` to decision). It never opens a socket and has no async
runtime. The host language sends the request.

## Consequences
- Small (two dependencies), testable offline with golden fixtures, trivially
  bindable.
- Every consumer must supply an HTTP client. Rust users need glue code that
  the README does not yet show (see `future-work.md`, "Release and consumer
  readiness").
