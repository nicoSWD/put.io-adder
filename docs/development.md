# Developing and testing without an account

`make` builds the release binary and `make help` lists the common tasks (`make check` is what CI runs for Rust, `make ci` adds the release-profile lint, the tool tests and the API-spec digest). `make demo VIEW=<view> THEME=<theme>` runs the views below.

Debug builds have a **demo mode**: sample transfers and a sample account, no network, no sign-in.

```sh
PUTIO_ADDER_DEMO=list  cargo run -p putio-adder     # a list with running, queued, done and failed transfers
PUTIO_ADDER_DEMO=empty cargo run -p putio-adder     # signed in, nothing yet
PUTIO_ADDER_DEMO=collapsed cargo run -p putio-adder # the list closed (header only)
PUTIO_ADDER_DEMO=default-app cargo run -p putio-adder # the "make this your default app?" question
PUTIO_ADDER_DEMO=login cargo run -p putio-adder     # the sign-in panel with a fake code
PUTIO_ADDER_DEMO=prefs cargo run -p putio-adder     # the list with Preferences open
PUTIO_ADDER_DEMO=folders cargo run -p putio-adder   # Preferences on the folder chooser page
PUTIO_ADDER_DEMO=menu cargo run -p putio-adder      # the "..." menu open (menu-update: with a new version waiting)
PUTIO_ADDER_DEMO=sort cargo run -p putio-adder      # the sort drop-down open
PUTIO_ADDER_DEMO=card-menu cargo run -p putio-adder   # a card's "⋯" menu open (also card-menu-seeding, -failed, -done)
PUTIO_ADDER_DEMO=details cargo run -p putio-adder     # the details panel (also details-downloading, details-failed, details-long)
PUTIO_ADDER_DEMO=files cargo run -p putio-adder     # the Files tab (also files-empty, files-loading, files-failed, files-search, files-deep, files-long, file-card-menu)
PUTIO_ADDER_DEMO=file-share cargo run -p putio-adder  # the sharing card (also file-share-empty, file-share-denied); the Files views mark two entries as shared
PUTIO_ADDER_DEMO=file-rename cargo run -p putio-adder # a Files card (also file-new-folder, file-move, file-delete, file-delete-folder)
PUTIO_ADDER_DEMO=feeds cargo run -p putio-adder     # the Feeds tab (also feeds-empty, feeds-loading, feeds-failed, feed-card-menu)
PUTIO_ADDER_DEMO=feed-add cargo run -p putio-adder  # the add-feed form (also feed-edit, feed-delete: the question before deleting)
PUTIO_ADDER_LOCALE=de|es PUTIO_ADDER_DEMO=feeds cargo run -p putio-adder   # any view in German or Spanish
PUTIO_ADDER_DEMO=update cargo run -p putio-adder    # an installable update on offer (also update-installing, -ready, -failed, -manual, prefs-update)
PUTIO_ADDER_DEMO_TOGGLE=3000 PUTIO_ADDER_DEMO=list cargo run -p putio-adder   # opens and closes the list every 3 s (the window animation)
PUTIO_ADDER_THEME=dark|light|system                 # override the saved theme (any debug run)
```

A demo run is **isolated from the real app**:

| It does not | So |
|---|---|
| use the OS keychain (a throw-away in-memory store instead) | no Keychain prompt, ever (macOS asks again whenever the code signature of a rebuilt app changes) |
| read or write the settings file (defaults are used) | your real preferences are never touched |
| use the single-instance socket | it runs next to the real app and never forwards to it |
| open any network connection (every worker job is dropped, including the update check) | safe to click around |

The demo code is compiled out of release builds (`cfg(debug_assertions)`), so a released app cannot be put into
demo mode. Check: `strings put.io\ adder.app/Contents/MacOS/putio-adder | grep PUTIO_ADDER_DEMO` finds nothing.

Other debug-only helpers: `PUTIO_ADDER_ICON_DUMP=<path>` writes the image macOS holds as the app icon (TIFF) so a
test can check the light/dark icon switch without looking at the Dock.

## Running the real app locally

`tools/bundle-macos.sh && open "target/put.io adder.app"` builds an ad-hoc signed bundle. Every rebuild changes its
signature, so macOS asks for Keychain access again; click *Always Allow*. (A self-signed development certificate would
keep the signature stable; none is set up.)

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
for t in tools/test_*.py; do python3 -I "$t"; done
```

## Screenshots

The README screenshots (`docs/images/screenshot-light.png` and `-dark.png`, `screenshot-files-light.png` and `-dark.png`, `screenshot-feeds-light.png` and `-dark.png`) are the demo list
(`PUTIO_ADDER_DEMO=list`), files (`files`) and feeds (`feeds`) with
`PUTIO_ADDER_THEME=light|dark`, captured as a window-only 2x PNG (`screencapture -o -l <window id>`). The demo data is
fictional (public-domain film titles), so nothing personal is in them. Retake them when the look changes.
