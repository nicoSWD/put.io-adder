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
- **Transfer details** (`state/detail.rs`) are fetched on their own while the panel is open, every three seconds, because the transfers list is only polled every twenty. One request is in flight at a time, an answer for a panel that has been closed or switched to another transfer is thrown away, a failed fetch keeps what was shown and waits longer, and a 404 (the transfer was cleared elsewhere) ends the panel. The panel's folder and feed rows need a second look-up: the folder's name is asked of put.io once and remembered, the feed's title comes from the Feeds list when it has been loaded. *Show in Files* only appears when both the folder and its name are known, since the Files tab needs both to put the folder in its breadcrumb.
- **Uploads** (`state/uploads.rs`): files dropped on the window, or chosen with the *Upload* button (the system file dialog, `putio-platform::file_dialog`), go into the folder the Files tab shows. A failed upload keeps its row with the reason (the status line notice is one line and fades) until it is put away. The shell looks at the disk and tells the core what each item is; the core starts one upload per regular, non-empty file (at most 100 per drop; folders and empty files are skipped and said so), and a `.torrent` is added as a transfer instead. The body is never held in memory: the transport sends the multipart head, then the file read from disk, then the tail, with a `Content-Length` (not chunked), so a file of any size costs a few kilobytes of memory. The file's length is checked when the request is built and again as it is read, so a file that changes meanwhile fails (as a file error, not as "offline") instead of uploading something else. Uploads have a lane of their own in the worker (one at a time, in order), so a big file does not hold up torrents or streaming; each reports progress (at most a thousand times) and has a flag that stops it, which signing out sets for all. The whole request may take up to 12 hours. A 401 is a failed upload, never a sign-out.
- **Sharing** (`state/shares.rs`) is the sharing card of a file plus a set of shared files that marks them in the Files list. The card asks put.io who the file is shared with and who the friends are, shares only on a deliberate request (one request at a time, with friends picked by name from the list put.io gave), and asks again afterwards so that what is shown is what put.io says, not a guess. The set of shared files is fetched while the Files tab is open. **A 401 from any of these calls is shown as "put.io does not let this app do that" and never signs the user out**: put.io answered an account-settings write with 401 on a working token (see `docs/spikes.md`), and the polls that run all the time are what notice a session that really ended. After a 401 the marks are not asked for again.
- **Friends** (`state/friends.rs`) are a card opened from the "..." menu: the friends, the requests waiting, a field to ask somebody, and buttons to accept, decline or remove. It works like sharing: a change is a deliberate request, one at a time, both lists are asked for again after it, a 401 is a refusal and never a sign-out, and removing a friend asks first because put.io takes the files shared with all friends out of that friend's folder (the question says so; that is the spec's own note).
