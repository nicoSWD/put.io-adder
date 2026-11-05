# Developing and testing without an account

Debug builds have a **demo mode**: sample transfers and a sample account, no network, no sign-in.

```sh
PUTIO_ADDER_DEMO=list  cargo run -p putio-adder     # a list with running, queued, done and failed transfers
PUTIO_ADDER_DEMO=empty cargo run -p putio-adder     # signed in, nothing yet
PUTIO_ADDER_DEMO=login cargo run -p putio-adder     # the sign-in panel with a fake code
PUTIO_ADDER_DEMO=prefs cargo run -p putio-adder     # the list with Preferences open
PUTIO_ADDER_DEMO=folders cargo run -p putio-adder   # Preferences on the folder chooser page
PUTIO_ADDER_DEMO=menu cargo run -p putio-adder      # the "..." menu open (menu-update: with a new version waiting)
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
