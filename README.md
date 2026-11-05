# putio-adder-rs

A small cross-platform app (macOS, Windows, Linux; x86_64 and arm64) that registers as the handler for
`magnet:` links and `.torrent` files and sends them to your [put.io](https://put.io) account. It also shows your
transfers, notifies you when they finish and can stream them in VLC.

Rust rewrite of the original Objective-C *put.io adder*. **Work in progress**: see `docs/` for the architecture and plan.
