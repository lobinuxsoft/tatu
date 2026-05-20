# tatu-launcher

Steam compatibility tool that delegates to a real Proton while
substituting `tatu-bridge.exe --launch <game.exe> --target-exe <name>`
for games the user has opted into. This is what makes the Aurora-style
co-launch reachable from the Steam UI — pick "Tatu Launcher" in
*Properties → Compatibility* and the bridge handoff happens
transparently inside one Proton invocation (shared SLR container,
shared wineserver).

## Layout

```
tools/tatu-launcher/
├── toolmanifest.vdf            # Steam tool manifest (commandline → tatu-launcher.sh)
├── compatibilitytool.vdf       # Steam compat-tool registration
├── tatu-launcher.sh            # Thin shell wrapper: LD_PRELOAD scrub → exec binary
├── tatu-launcher.toml.example  # User config template
└── README.md                   # This file
```

The Rust binary `tatu-launcher` ships next to these files after build.
Drop-in install layout under Steam:

```
~/.steam/root/compatibilitytools.d/tatu-launcher/
├── toolmanifest.vdf
├── compatibilitytool.vdf
├── tatu-launcher.sh
├── tatu-launcher           # Linux ELF (parses config, resolves Proton, rewrites argv)
└── tatu-bridge.exe         # Win32 PE built via mingw-w64 (the bridge from Phase 1)
```

## Config

`~/.config/tatu/launcher.toml` — see `tatu-launcher.toml.example`.
Global `default_proton` + optional per-appid `[games.<id>]` table with
`proton`, `target_exe`, `tatu_enabled`. Games not listed (or with
`tatu_enabled = false`) passthrough to `default_proton` unmodified.

## Verbs

Steam can call any of `waitforexitandrun`, `run`, `getcompatpath`,
`getnativepath`. The launcher rewrites argv only for
`waitforexitandrun` on opted-in appids; every other verb is delegated
to the real Proton verbatim so Steam's compat queries keep working.

## Phase 6/7 integration

The tracker UI will write to this same TOML when the user toggles
"Enable Tatu backend" per game. No schema migration needed — Phase 2
defines the file shape; later phases just gain a writer.
