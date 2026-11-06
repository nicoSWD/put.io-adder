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
- **Avatars:** the header shows the account's initial. A Gravatar would need a hash of the e-mail sent to a third party, so there is none.
- **Updates** compare semantic versions; installing one is described in [updates.md](updates.md).
- **RSS feeds** live on put.io, which watches them and turns new items into transfers; the app only manages the list (`state/feeds.rs`). The list is fetched when the Feeds tab is open and not otherwise, a form is checked before anything is sent (`feeds.rs`), and what put.io refuses is explained in the user's language. A feed's address often carries a private key, so only its host is ever shown and only a web address is ever copied.
- **The window's tabs** (`ui/tabs.rs`) are planned before they are painted: counts go first, then labels are cut, so the longest German or Spanish word cannot push the controls beside it out of place. The tests draw each piece in every language at the narrowest window and look at where the text lands.
- **Files** are a list the app only reads and changes through put.io's calls (`state/files.rs`): the folder being shown, the way to it, and a search. An answer for a folder the user has already left is thrown away, the rows of the old folder go at once (they would be clickable until the new ones arrive), and every change is followed by a fresh listing. A name is checked before it is sent, and deleting asks first because put.io has no trash.
- **Extracting archives** is put.io's work; the app asks for it, polls the list of running extractions every two seconds while there is one (and not otherwise), and says how it went. A password goes to put.io in the request only: the type that carries it prints as `Secret(…)` everywhere, so an event, an effect or a job can be logged without leaking it, and the card that asks for it never closes on a stray click.
