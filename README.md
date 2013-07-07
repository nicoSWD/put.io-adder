Put.io Magnet Handler for OS X
==============================

"Put.io adder" is a small OS X app, that adds a new handler for "magnet:" URL schemes, so that every time you click
on a "magnet:" link, the app opens, and adds the clicked link to your [put.io](http://put.io/) download queue.

**IMPORTANT**
I added [AFNetworking](https://github.com/AFNetworking/AFNetworking) and [SSKeyChain](https://github.com/samsoffes/sskeychain) as submodules. So if you want to clone the repo, please use

`$ git clone --recursive git@github.com:nicoSWD/put.io-adder.git`

Please [report issues](https://github.com/nicoSWD/put.io-adder/issues) you may encounter. Pull requests are also very welcome!

Follow me on Twitter: [@nicoSWD](https://twitter.com/nicoSWD)!

**NOTES**
- The app requires you to have a [put.io](http://put.io/) account. They're probably not free anymore.
- The app requires KeyChain Access to store put.io's OAuth Token. Denying access might prevent the app from working.

**BINARIES**
- Newest
  - [put.io adder v1.3](http://cl.ly/Q6ak) (MD5 checksum: 477afa6d458347c6d24b9af0a53a2ab8)
- Older releases
  - [put.io adder v1.2](http://cl.ly/Q5Vx) (MD5 checksum: c82b0c5007c7dd2cd56ad64eb911112f)
  - [put.io adder v1.1](http://cl.ly/Q4uX) (MD5 checksum: f0dad56b625b78d2ef503f3113ff4e55)
  - [put.io adder v1.0](http://cl.ly/Q4r2) (MD5 checksum: 826fb0c5280783bc2b9d08e921255b82)

**SCREENSHOTS**

![alt tag](http://f.cl.ly/items/2l1Q082D0N470C2a0V22/Screen%20Shot%202013-07-06%20at%203.58.53%20PM.png)
![alt tag](http://f.cl.ly/items/461e2y412A3h1F3E1a0A/Screen%20Shot%202013-07-06%20at%203.59.57%20PM.png)
![alt tag](http://f.cl.ly/items/1x243e432W0R1d2n2G0n/Screen%20Shot%202013-07-06%20at%203.59.15%20PM.png)

**CHANGELOG**
- v1.3
  - Displays account info
  - Added link to GitHub repo in menu bar
- v1.2
  - Now displays the "display name" instead of the magnet URL
  - Moved code a bit around
  - Better error control
- v1.1
  - Now compiles with Xcode 4
  - Compatible with OS X < 10.8
  - New Design
- v1.0
  - Initial release

**MIT/X11 LICENSE**

Copyright (C) 2013 Nicolas Oelgart

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
