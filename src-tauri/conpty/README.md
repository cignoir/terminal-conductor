# Bundled ConPTY

`conpty.dll` and `OpenConsole.exe` are the pseudoconsole from the
Windows Terminal project, installed next to `terminal-conductor.exe`.
portable-pty loads a `conpty.dll` found there in preference to the one
built into Windows, and that DLL starts the `OpenConsole.exe` beside it.

Why: the ConPTY built into Windows measures an emoji joined with ZWJ
(for example U+1F647 U+200D U+2640 U+FE0F) as five cells, while Claude
Code and other CLIs draw it as two. The mismatch shifts the rest of the
screen on every redraw. This version measures grapheme clusters as
single characters, as xterm.js does with the unicode-graphemes addon.

| | |
|---|---|
| Source | NuGet `Microsoft.Windows.Console.ConPTY` 1.25.260930003 |
| Files | `runtimes/win-x64/native/conpty.dll`, `build/native/runtimes/x64/OpenConsole.exe` |
| Signature | Authenticode, CN=Microsoft Corporation (checked on import) |
| License | MIT, see `LICENSE`; third-party notices in `NOTICE.md` (both from github.com/microsoft/terminal) |

SHA-256:

```
feeef341d891643c62d30b6b07800bc70f0bc148f44cb8c3bee84aa557ae805a  conpty.dll
3d66b23d0a71bb8eed2b77edc8b9df9bf54ce6c8fb74c863a30e760997f80586  OpenConsole.exe
```

To update, download the package from nuget.org, check the Authenticode
signature of both files, copy them here, and update the version and
hashes above.
