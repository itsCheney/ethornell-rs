# src/

On this branch (`mikage`, the Mikage player's fork) `src/` keeps only the
architecture decision records in `adr/`. Upstream's `src/raw`, `src/handlers`,
`src/tools`, `src/engine` and `src/include` (the decompiled original engine,
its C restoration and the tools that produce them) are left out; none of it is
built. Comments in the Rust that point at those files refer to upstream
[xmoezzz/ethornell-rs](https://github.com/xmoezzz/ethornell-rs).
