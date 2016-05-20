## Put.io magnet: and .torrent Handler for OS X

![alt tag](http://f.cl.ly/items/0K420o1W192i0i2u0r31/Screen%20Shot%202013-11-07%20at%2017.51.05.png)

*Put.IO Adder* is a small open source app for OS X, that registers as new handler for `magnet:` URI schemes, so that every time you click on a magnet link, it will automatically add it to your [put.io](http://put.io/) download queue.

Follow me on Twitter: [@nicoSWD](https://twitter.com/nicoSWD)!

**FEATURES**
- Automatically add `magnet:` links and `.torrent` files to your download queue by clicking on them
- Stream finished movies directly from the app (requires [VLC](https://www.videolan.org/vlc/index.html) to be installed in /Applications)
- View current transfers and their download status
- `Notification Center` notifications on finished downloads
- etc...

**DOWNLOADS**
- Newest
  - [put.io adder v3.0](https://github.com/nicoSWD/put.io-adder/releases/tag/v3.0)
- Older releases
  - [put.io adder v2.9](https://github.com/nicoSWD/put.io-adder/releases/tag/v2.9)
  - Other releases can be found [here](https://github.com/nicoSWD/put.io-adder/releases/)

**NOTES**
- The app requires you to have a [put.io](http://put.io/) account. They're ~~probably~~ not free anymore.
- The app requires KeyChain Access to store put.io's OAuth Token. Denying access will prevent the app from working.
- To disable NotificationCenter, go to System Preferences -> Notifications -> Scroll to Put.IO Adder, and make your changes.
- As of now, only movies can be streamed that are not inside any folders. The app is not yet smart enough to find files inside folders.
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


**SCREENSHOTS**

![alt tag](http://f.cl.ly/items/0c1W0O3l0b1F1y3u3q1S/Screen%20Shot%202014-01-26%20at%2021.32.05.png)
![alt tag](http://f.cl.ly/items/0m3d2a1k1Y460v2y292L/Screen%20Shot%202013-10-28%20at%2022.06.57%20copy.png)
![alt tag](http://f.cl.ly/items/3Q2t3g311h442c3o1z0K/Screen%20Shot%202013-07-19%20at%209.31.28%20PM.png)
![alt tag](http://f.cl.ly/items/0Y3n24003K0Z2c2X2s2i/Screen%20Shot%202013-07-19%20at%209.31.39%20PM.png)
![alt tag](http://f.cl.ly/items/420N3Y1w3N0h3U27451I/Screen%20Shot%202013-07-19%20at%209.31.48%20PM.png)

**MIT/X11 LICENSE**

Copyright (C) 2013-2016 Nicolas Oelgart

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
