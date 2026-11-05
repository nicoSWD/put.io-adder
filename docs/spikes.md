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
