## Put.io magnet: and .torrent Handler for OS X
![screenshot](https://s3.amazonaws.com/f.cl.ly/items/362l1z432n3O2w1X2C2B/Screen%20Shot%202016-05-21%20at%2000.48.37.png)

*Put.IO Adder* is a small open source app for OS X, that registers as new handler for `magnet:` URI schemes, so that every time you click on a magnet link, it will automatically add it to your [put.io](http://put.io/) download queue.

Follow me on Twitter: [@nicoSWD](https://twitter.com/nicoSWD)!

**FEATURES**
- Automatically add `magnet:` links and `.torrent` files to your download queue by clicking on them
- Stream finished movies directly from the app (requires [VLC](https://www.videolan.org/vlc/index.html) to be installed in /Applications)
- View current transfers and their download status
- Double click transfers to open them on put.io
- `Notification Center` notifications on finished downloads
- etc...

**DOWNLOADS**

| Version | Downloads |
|:----------:|:---------------:|
| **[put.io adder v3.0.2](https://github.com/nicoSWD/put.io-adder/releases/download/v3.0.2/put.io-adder-v3.0.2.zip)** | ![total](https://img.shields.io/github/downloads/nicoSWD/put.io-adder/total.svg?maxAge=25920)

  (Older releases can be found [here](https://github.com/nicoSWD/put.io-adder/releases/))

**NOTES**
- The app requires you to have a [put.io](http://put.io/) account. They're ~~probably~~ not free anymore.
- The app requires Keychain Access to store put.io's OAuth Token. Denying access will prevent the app from working.
- To disable Notification Center, go to System Preferences -> Notifications -> Scroll to Put.IO Adder, and make your changes.
- There are three ways to upload `.torrent` files to your account:
    - Drag the .torrent file onto the app icon
    - Make *put.io adder* your default handler for .torrent files
    - Or right click the torrent and pick "Open with" > put.io adder

Please [report issues](https://github.com/nicoSWD/put.io-adder/issues) you may encounter. Pull requests are also very welcome!

**CONTRIBUTE**

[AFNetworking](https://github.com/AFNetworking/AFNetworking), [PutioKit](https://github.com/PutioKit/PutioKit), and [SSKeyChain](https://github.com/samsoffes/sskeychain) have been added as submodules. To clone the repo in its entirety, use:

`$ git clone --recursive git@github.com:nicoSWD/put.io-adder.git`


All contributions are greatly appreciated. If you want to help, send a pull request, or open a [new issue](https://github.com/nicoSWD/put.io-adder/issues/new) with a feature request.

Some ideas for improvement are:

- Check if *put.io adder* is the default handler for magnet links and torrent files on startup. *~/Library/Preferences/com.apple.LaunchServices.plist* needs to be parsed and/or modified for that. If it's not, an alert asking to change that should be fired.
- A ["Default folder preference"](https://github.com/nicoSWD/put.io-adder/issues/3) option could be added.
- An option to download finished transfers to the computer through the app would be nice.
- A way to browse through existing files and folders on *put.io*.
- More languages.
- Settings could be moved to a new preference pane in *System Preferences*.
- ~~Design improvements.~~
- ~~A way to stream finished files through the app.~~
- ~~Authentication could be improved by adding a new custom URI scheme. When authenticating, the browser should open and go to *put.io's* authentication page, and a callback to something like *putio://callback* would open the app and pass over the OAuth token. Just like [mikker's Putter app does](https://github.com/mikker/Putter.app/blob/master/Putter/BBAppDelegate.m).~~ (Added in v2.7)
- ~~Besides Notification Center, a badge could be added to the app icon, indicating the number of finished downloads. Or the download progress could be shown there.~~
- etc...
