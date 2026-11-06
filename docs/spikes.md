# Phase 0 spike results

Both spikes ran on macOS with eframe 0.36.2 / winit 0.30.13 (the DegenView stack).

## macOS intake (magnet + .torrent), cold and warm launch

Apple Events arrive as `GURL` (magnet) and `aevt/odoc` (.torrent, delivered as a `file://` URL string
per item). When the handler is installed matters:

| Install point | Cold magnet | Warm magnet | Cold .torrent | Warm .torrent |
|---|---|---|---|---|
| `main`, before `run_native` | ok | ok | **lost** | **lost** (AppKit replaces the `odoc` handler) |
| eframe creation callback | **lost** | ok | **lost** | ok |
| `main` **and** re-installed on `NSApplicationWillFinishLaunchingNotification` | ok | ok | ok | ok |

Decision: install both handlers in `main`, and install them again from a
`NSApplicationWillFinishLaunchingNotification` observer. Events are queued into a channel the UI drains, so
nothing depends on the UI being ready. The binding uses `msg_send!` with `u32` four-char codes, so no
extra `objc2-core-services` dependency is needed. The `NSAppleEventManager` does not retain its handler:
the handler object must live for the whole process.

## Window collapse / expand

`ViewportCommand::InnerSize` resizes a window created with `with_resizable(false)` (86 px <-> 300 px,
550 px wide) on macOS. The header strip, gold bar and rotating chevron render as designed using only the
egui painter. Still to verify on Windows and Linux in CI/manual runs.

## Still to probe against the live API (needs a real token)

Phase 0 spike 1 is open. Everything the client assumes about behaviour the spec does not state:

1. ~~`GET /account/info`: is `disk` still returned?~~ **It depends on the account** (observed 2026-11-05): one account came back without a usable `disk` (the bar fell back to monthly bandwidth), another showed "91.6 GB left", so `disk` is sent for accounts with a quota. Keep `disk` optional, as modelled; the header shows the quota pill when it is there and the bandwidth outline when it is not.
2. ~~`GET /oauth2/oob/code?app_id=711`: does OOB work for app id 711, which was registered for the implicit grant?~~ **Yes**: the app got a device code with app id 711 on its first launch (observed 2026-11-05; the sign-in itself is still to be completed).
3. ~~**What does `GET /oauth2/oob/code/{code}` return before the user has linked the code?**~~ Sign-in completed end to end (observed 2026-11-05), so the pending state does not abort it. The exact pending body is still unrecorded; capture it as a fixture. Original question: The client
   assumes `200 {"oauth_token": null}` (or no field). If put.io answers with a 4xx instead, sign-in would be
   aborted on the first poll (`link_polled` treats a non-transient error as "code expired").
4. ~~Does put.io accept `Authorization: Bearer <token>`?~~ **Yes**: account info and the transfer list were fetched with it (observed 2026-11-05).
5. The error body for adding a duplicate transfer (`error_type`).
6. `GET /files/{id}/url` on a folder.
7b. **Sharing** (`GET /files/shared`, `GET /files/{id}/shared-with`, `GET /friends/list`, `POST /files/share`, `POST /files/{id}/unshare`) is built on the spec and put.io's TypeScript SDK and has not been run against a live account. Unknown: whether this app's token may call them at all (an account-settings write was refused with 401, see below under the settings note if present), what `shared-with` lists after sharing with `everyone` (the card then shows whatever put.io returns), and whether `shares=` takes several ids comma-separated (the card only ever sends one).
7a. ~~**`POST /account/settings`: can this app change account settings?**~~ **No** (observed 2026-11-06, one real account, token otherwise fine). Changing `default_download_folder` was answered with **401**, both as a JSON body (as put.io's TypeScript SDK, putdotio/putio-sdk-typescript `src/domains/account.ts`, sends it) and as a form field. The vendored spec has the GET only (`GET /account/settings`; no `trash_enabled`, and `AccountInfo` has no `settings`). Why is unknown: most likely this app's token (app id 711, registered for the implicit grant) is not allowed to write account settings. A first attempt treated that 401 as an expired session and signed the user out; **a 401 is not proof the session ended** for any call that is not polled all the time, so a write that is refused with 401 should be shown as a refusal. Not tried: `trash_enabled` (probably the same), and whether `/account/info` carries `settings`. The feature was built (a trash switch and a default-folder chooser in Preferences) and removed again; see PR #56.
7. Does `POST /transfers/cancel` return an empty body or `{"status":"OK"}`? (The spec shows no content; the client accepts both.)

Replace `crates/putio-api/tests/fixtures/*.json` with scrubbed recordings afterwards.

7c. **Friends** (`GET /friends/list`, `GET /friends/waiting-requests`, `POST /friends/{username}/request|approve|deny|unfriend`) is built on the spec and has not been run against a live account. Unknown: whether this app's token may use them (same doubt as sharing), the error type put.io gives for an unknown user name (the card shows its message as it is), and whether a name needs the exact case. The calls send an empty POST body, as the spec lists no fields.

7d. **Uploading any file** (`POST /files/upload` on `upload.put.io`, multipart with `file`, `filename` and `parent_id`) is built on the spec and put.io's description and has not been run against a live account. Unknown: the largest file that endpoint takes in one request (put.io has a separate resumable upload for very large files, which is not used), whether it accepts a body with `Content-Length` and no chunking from the start (the loopback test shows the app sends one), what it answers for a name that already exists, and whether this app's token may upload non-torrent files at all.

7e. **Resumable upload (tus)**, used from 1 GiB, is built on the code of put.io's Go client (`go-putio` `upload.go` and `client.go`: endpoint `https://upload.put.io/files/`; metadata keys `name`, `parent_id`, `no-torrent`, `overwrite`; `HEAD` for `Upload-Offset`; `PATCH` with `application/offset+octet-stream`; `204` with `Putio-File-Id`). No put.io documentation of it was reachable, and it has not been run against a live account. Unknown: how put.io authenticates this endpoint (the app sends the same `Authorization: Bearer` as everywhere), whether it wants a `Tus-Resumable` header (the Go client sends none, so the app sends none), the largest file and part it takes, what a name that already exists does with `overwrite=false`, and how long an unfinished upload is kept (the app does not keep the address across a restart).
