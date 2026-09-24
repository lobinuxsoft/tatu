# Vendored Goldberg Steam Emulator (gbe_fork)

`steam_api64.dll` and `steam_api.dll` in this folder are the **regular**
build from [Detanup01/gbe_fork](https://github.com/Detanup01/gbe_fork)
release **`release-2026_08_23`**, licensed LGPL-3.0 (`LICENSE` in this
folder). They are drop-in replacements for Steam's own `steam_api(64).dll`
that let a Steam-wrapper-only game (`Preservability::Easy`) run through
Proton without the Steam client — see #199.

Pinned deliberately: no runtime download, no build step. To pick up a
newer gbe_fork release, replace both files and this note with the new tag.

Not vendored: the `experimental/` variant (overlay + loader executables),
`steam_settings/` templates, and `generate_interfaces` — out of scope for
now, see #199's discussion.
