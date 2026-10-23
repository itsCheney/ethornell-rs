# The Mikage branch

`mikage` is the branch of [itsCheney/ethornell-rs](https://github.com/itsCheney/ethornell-rs)
that [Mikage](https://github.com/itsCheney/Mikage-Next) builds its BGI engine
from (`Engine/EthornellRuntime/Source`). It follows upstream
[xmoezzz/ethornell-rs](https://github.com/xmoezzz/ethornell-rs) `main` and adds
a few small commits:

- `LICENSE` with the MPL-2.0 text the workspace declares, and
  `license.workspace = true` in the crates.
- Upstream's decompiled engine (`src/raw`, `src/handlers`, `src/tools`,
  `src/engine`, `src/include`) and two save files from a game
  (`crates/ethornell-app/BGI.gdb`, `BGI0000.cad`) removed; nothing built used them.
- The user root may sit directly inside the resource root (Mikage keeps saves
  in `<game>/savedata`). That directory is then invisible to resource reads,
  listings and the archive scan (`file_policy.rs`).
- On iOS a game folder whose parent folders can't be opened (the app
  container) is opened by its full path (`file_policy.rs`).
- Launcher executables: `ethornell_mikage_set_launcher` names a patch's exe in
  the game folder for the next session. The files a translation patch packed
  into it with Enigma Virtual Box (`crates/ethornell-archive/src/evb.rs`, with
  an aPLib unpacker) win over every archive entry of the same name, as loose
  files in the game folder do on Windows. They are read from the exe on
  demand; nothing is extracted.
- No GitHub workflows: upstream's CI and releases don't run on this fork; Mikage builds the library in its own CI.
- `ethornell_mikage_screen_size`: the game's own screen size, for the host's layout.
- `ethornell_mikage_set_log_callback` (`crates/ethornell-mobile/src/mikage.rs`,
  `platform/ios/include/ethornell_mikage.h`): engine warnings, errors and panic
  messages go to the host's log.

To update: `git fetch upstream && git merge upstream/main`.
