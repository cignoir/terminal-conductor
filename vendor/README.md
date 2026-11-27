# Vendored crates

## portable-pty 0.9.0

Copied from crates.io (MIT, `LICENSE.md`) and patched in through
`[patch.crates-io]` in the workspace `Cargo.toml`. The only change is in
`src/win/psuedocon.rs`: `CreatePseudoConsole` also gets
`PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES` (0x8).

With that flag, the bundled `conpty.dll` (see `src-tauri/conpty/`) starts
OpenConsole with `--textMeasurement graphemes`, as Windows Terminal does.
Without it OpenConsole keeps its buffer in the legacy console format: a wide
character or an emoji becomes two separate one-cell characters, and a ZWJ
emoji sequence such as `🙇‍♀️` can be counted as five cells when OpenConsole
repaints the screen from its buffer. The terminal (xterm.js with the
grapheme addon) counts two, so everything after the emoji on that row, and
on the rows below it, moved left.

On the inbox conhost (when `conpty.dll` is missing) the flag is ignored.

To update: copy the new version from `~/.cargo/registry/src/*/portable-pty-*`,
drop `.cargo-ok`, `.cargo_vcs_info.json` and `Cargo.lock`, and apply the same
change again.
