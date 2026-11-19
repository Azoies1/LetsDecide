# 0006: Keys live in memory only

- Status: Accepted
- Date: 2026-11-19

## Context
The library handles provider API keys and may run in logs-heavy environments.

## Decision
Keys are passed in `Config` and held in memory. `Config`'s `Debug` redacts
them. Errors never include keys or request bodies. No telemetry and no network
calls from the core. The library does not read environment variables; callers
do.

## Consequences
- Low leak surface.
- Docs must show callers how to load keys safely (env vars, `.env` kept out of
  git). Browser code cannot hold a secret: call through a backend.
