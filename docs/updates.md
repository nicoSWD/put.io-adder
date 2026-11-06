# Updates

put.io adder can replace itself with a newer release from GitHub: it shows an **Update now** button, downloads
the release file for this computer, unpacks it next to the app, and swaps it in when you press **Restart to
finish**. The design follows the updater of the sibling project DegenView, with deliberately fewer checks (see
"What is not checked").

## How it works

1. **Check.** `GET /repos/nicoSWD/put.io-adder/releases/latest` on start-up, then every 6 hours (30 minutes
   after a failed check) while the app stays open, if "Check for updates" is on. GitHub's `latest` is the
   newest published release that is not a draft and not a pre-release, so only the **stable** channel is
   followed. The version is compared with semver (`0.10.0 > 0.9.0`).
2. **Offer.** An update is *installable* only when all of this holds (`putio_core::update`):
   - the release was published by `github-actions[bot]` or `nicoSWD`;
   - it has the file this build expects, with exactly this name: `putio-adder-<tag>-macos-arm64.zip`,
     `…-macos-x64.zip`, `…-windows-x64.zip`, `…-windows-arm64.zip`, `…-linux-x86_64.tar.gz` or
     `…-linux-aarch64.tar.gz`;
   - the file's address is exactly `https://github.com/nicoSWD/put.io-adder/releases/download/<tag>/<name>`;
   - its size is between 1 byte and 128 MiB;
   - this copy can replace itself (below).

   Anything else is offered as **View release** with the reason in the tooltip.
3. **Download and unpack** (`putio_platform::update`, on a worker thread, with a progress pill):
   HTTPS only, GitHub hosts only on every redirect hop (`github.com`, `*.githubusercontent.com`), no
   credentials, at most 128 MiB, 10 minutes. The size must match what GitHub published (a guard against a cut-off
   download, not a checksum). Unpacking is bounded and structural:
   - **macOS**: the zip's directory is checked first (entry count, declared sizes, no `..`, absolute or
     backslash names, no ZIP64), then `ditto` unpacks it with its output size watched and a 2-minute limit. The
     bundle must have the id `es.nico.putio-adder` and `Contents/MacOS/putio-adder`, no other executable and no
     link that leaves it.
   - **Windows**: the zip's directory is checked, then only `putio-adder.exe` at the root is read, under a fixed
     output name and a size cap. It must be a PE image for this CPU.
   - **Linux**: only `putio-adder-<tag>-linux-<arch>/putio-adder` is read from the tarball, under a fixed output
     name, with caps on entries and decompressed bytes. It must be a 64-bit ELF for this CPU.
4. **Restart.** The old version is renamed into the hidden staging folder `.putio-adder-update` next to the app and
   the new one renamed into its place (two atomic renames on one volume; if the second fails the first is undone,
   and if that fails too the message says where the old copy is). Then:
   - macOS: a detached `sh` helper waits for this process to exit and runs `open -n` on the app.
   - Windows/Linux: the new executable is started with `--relaunch-after <pid>`; it waits (up to 60 s) for the
     single-instance lock, so two instances never run at once.

   The next start deletes the staging folder (and with it the previous version).

## Where it will not install

- A **development build** (`cargo run`, anything under the source tree, including the bundle `make app` writes to `target/`). The message says so.
- An app in a folder the user cannot write to (`Program Files`, a read-only disk image, `AppTranslocation`):
  **View release**, with the reason.
- macOS builds that are not inside a `.app` bundle.
- Any platform without a matching release file.

## What is not checked

This is a decision, not an oversight. The updater does **not** verify:

- a **checksum** of the file (GitHub publishes a SHA-256 for every upload; it is not compared);
- the **signature of the release tag**, if it has one;
- a **code signature** (macOS Developer ID / Team ID, notarization, Authenticode).

So the trust placed in an update is: the TLS connection to GitHub, the fact that the release is published
under this project's account, and the structural checks above. Anyone who can publish a release or replace an
asset under `nicoSWD/put.io-adder` can ship code to every user who presses *Update now*. Turn on **immutable
releases** in the repository settings, protect the account with a hardware key, and keep the release workflow
the only thing that uploads assets. If you want the missing checks later, the natural places are
`putio_platform::update::prepare_with` (compare `asset.size` and a SHA-256 from the release's `digest`
field) and `putio_core::update::pick_asset`.

The release workflow still checks the tag is on `master` with passing CI, attests build provenance and publishes `SHA256SUMS`; people
who download by hand can verify those (SECURITY.md). The app just does not.

## Known gaps

- **Windows "Installed apps"** keeps showing the version the installer wrote after an in-app update (DegenView
  rewrites that registry value; this app does not). Uninstalling still works.
- **macOS asks for Keychain access again** after an update of an ad-hoc signed build (a release made without the
  Apple signing secrets, or a local `make app`): the signature changes with the binary, so the Keychain no longer
  recognises the app. Developer ID signed releases keep the same identity across updates.
- The Windows and Linux swap and relaunch run only on those systems; they are tested here with fixtures on
  macOS, and the `cfg(windows)` spawn code has no test at all.
- Restart is held back while a magnet link or `.torrent` is being sent (the pill stays disabled); a transfer
  that is merely downloading on put.io is unaffected.

## Changing the pipeline

The names and layouts above are a contract with the release workflow and the packaging scripts. A test
(`the_packaging_scripts_produce_the_files_the_updater_looks_for`) reads those scripts and fails when they drift.
If you rename a file or move the binary inside an archive, change `putio_platform::update` in the same commit.

## Trying it

`PUTIO_ADDER_DEMO=update|update-installing|update-ready|update-failed|update-manual|prefs-update` shows each
state without touching the network. A real end-to-end update needs two published releases; see the manual
checklist in docs/releasing.md.
