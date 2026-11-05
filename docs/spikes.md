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
2. ~~`GET /oauth2/oob/code?app_id=711`: does OOB work for the old implicit-grant app id?~~ **Yes**: the app got a device code with app id 711 on its first launch (observed 2026-11-05; the sign-in itself is still to be completed).
3. ~~**What does `GET /oauth2/oob/code/{code}` return before the user has linked the code?**~~ Sign-in completed end to end (observed 2026-11-05), so the pending state does not abort it. The exact pending body is still unrecorded; capture it as a fixture. Original question: The client
   assumes `200 {"oauth_token": null}` (or no field). If put.io answers with a 4xx instead, sign-in would be
   aborted on the first poll (`link_polled` treats a non-transient error as "code expired").
4. ~~Does put.io accept `Authorization: Bearer <token>`?~~ **Yes**: account info and the transfer list were fetched with it (observed 2026-11-05).
5. The error body for adding a duplicate transfer (`error_type`).
6. `GET /files/{id}/url` on a folder.
7. Does `POST /transfers/cancel` return an empty body or `{"status":"OK"}`? (The spec shows no content; the client accepts both.)

Replace `crates/putio-api/tests/fixtures/*.json` with scrubbed recordings afterwards.
