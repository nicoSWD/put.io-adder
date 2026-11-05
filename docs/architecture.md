# Architecture

```
putio-adder (bin)  ─ egui UI, worker thread, wiring            [Phase 4]
   │  uses
putio-platform     ─ OS adapters behind traits (the only crate allowed `unsafe`)   [Phase 3]
   │  uses
putio-core         ─ pure state machine, validation, formatting, i18n, settings
   │  uses
putio-api          ─ typed put.io client over a `Transport` trait
```

## Functional core, imperative shell

`putio_core::state::update(&mut State, now, Event) -> Vec<Effect>` is the whole behaviour of the app.
It never touches the network, disk, clock or UI. The shell:

1. turns OS callbacks, timer ticks and API results into `Event`s,
2. calls `update`,
3. performs the returned `Effect`s (network calls on a worker thread, notifications, opening URLs, …).

Long-running work is tied to its result by an `OpId`, so a late or duplicated result cannot be
credited to the wrong request. Time is an argument (`now`, milliseconds), which is why polling,
back-off, auto-close and status-line expiry are all unit-tested without sleeping.

## Security boundaries

| Boundary | Rule | Where |
|---|---|---|
| Token | Zeroized, redacted `Debug`, only sent as `Authorization: Bearer`; never in URL, argv, logs | `putio-api::token`, `client` |
| Network | https only, closed host allowlist (`Host` enum), no redirects, size and time limits | `putio-api::http` |
| OS input (magnet, file URL, path) | Validated before it reaches the reducer; bounded; no control chars | `putio-core::magnet`, `intake` |
| `.torrent` bytes | Size cap + bencode sniff before upload | `putio-api::torrent` |
| URLs to open | Built in core from numeric ids / constants; the opener re-checks the prefix | `putio-core::state` |
| Server text shown in the UI | Control characters stripped, length bounded | `putio-core::state::reason` |
| Player | Signed IP-bound URL from `GET /files/{id}/url`, never the account token | `putio-api::client::file_url` |

## Design decisions

- **Sign-in:** the device (OOB) code flow, with no callback URL scheme to register or to forge.
- **Errors:** each failure says what went wrong (duplicate, offline, rate limited, signed out, server) instead of one catch-all message.
- **Operations** (cancel, retry, download, …) act on a transfer's *id*, never on a row position, so a list that changes under the user's hand cannot redirect an action.
- **Polling** starts right after sign-in and backs off on errors and rate limits.
- **Avatars:** a Gravatar needs a hash of the account e-mail, so it is opt-in; the default is the initial.
- **Updates** compare semantic versions; installing one is described in [updates.md](updates.md).
