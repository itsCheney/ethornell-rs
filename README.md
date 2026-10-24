# ethornell-rs

`ethornell-rs` is a reimplementation of the Ethornell / BURIKO General Interpreter (a.k.a. "BGI") visual novel engine.

<table>
  <tr>
    <td width="50%" align="center">
      <img src="./images/desktop-title.jpg" width="100%"><br>
      <b>Desktop</b><br>
      <sub>千の刃濤、桃花染の皇姫 体験版 (BGI 1.622), title screen.</sub>
    </td>
    <td width="50%" align="center">
      <img src="./images/desktop-scene.jpg" width="100%"><br>
      <b>Desktop</b><br>
      <sub>Character sprites, face window and message window.</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" align="center">
      <img src="./images/web.jpg" width="100%"><br>
      <b>Web</b><br>
      <sub>The same game in Chrome (WebAssembly, WebGL2).</sub>
    </td>
    <td width="50%"></td>
  </tr>
</table>

The runtime detects the native game identifier from the comparison in
`system.arc:ipl._bp`. If detection is unavailable, the identifier falls back to
`Tayutama2TV`. For example:

```sh
cargo run -p ethornell-cli -- run --game /path/to/sun_flower
```

`--game` locates the resource archives, loose files, and user data.

Without arguments (a double-click), `ethornell` asks for the game folder;
`ethornell /path/to/sun_flower` plays it directly.

Builds for Windows (x86_64), Linux (x86_64 and AArch64), FreeBSD (x86_64),
macOS (a universal `Ethornell.app`), Android (APK), iOS (unsigned IPA), the
web (a static page), Nintendo Switch (NRO) and PS Vita (VPK) are published on
[GitHub Releases](https://github.com/xmoezzz/ethornell-rs/releases).
The `pre-release` entry follows `main`; `v*` tags publish versioned releases.
See [`platform/`](platform/README.md) for building and using each platform,
including the Android and iOS apps and the console ports.

The window uses the game's icon from `icon.png`, an `.ico` file, or the icon
resources embedded in its Windows executable, in that order.

On PC (desktop and headless), display text defaults to **Auto**: at startup
the engine samples BCS scenario strings once to choose Shift-JIS (`cp932`) or
GBK (`cp936`). It does not detect each line. Short or inconclusive samples fall
back to Shift-JIS. Override with
`--text-encoding gbk` (or `cp932` / `auto`), for example:

```sh
ethornell run --game /path/to/game --text-encoding auto
```

`ETHORNELL_TEXT_ENCODING=auto|cp932|gbk` is also supported on desktop/headless;
the command-line choice takes priority. This selects graphics text buffers,
including dialogue and speaker names. Resource names and BP system strings
keep their existing Shift-JIS encoding. Auto selects one display code page for
the session; it does not select a different encoding for each file.

Auto detection is compiled only for PC builds. iOS, Android, Web, Vita and
Switch retain their existing explicit/fixed encoding configuration and do not
scan scenario resources for encoding detection.
