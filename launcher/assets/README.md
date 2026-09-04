# Vendored assets

## Fonts (`fonts/`)

- `Rajdhani-Bold.ttf`, `Rajdhani-SemiBold.ttf` — [Rajdhani](https://fonts.google.com/specimen/Rajdhani), SIL Open Font License. Same display font Tatu's own web frontend themes already use — kept for brand consistency, not a separate pick.
- `Inter-Regular.ttf` — [Inter](https://fonts.google.com/specimen/Inter) (variable font), SIL Open Font License. Body text.

Both pulled from Google Fonts' own repo (`github.com/google/fonts`, `ofl/` directory) at the tag current on 2026-08-26.

## Input prompt icons (`input_prompts/`)

- `steamdeck_button_{a,b,x,y}.png`, `keyboard_{enter,s,g,escape}.png` — from Kenney's [Input Prompts](https://kenney.nl/assets/input-prompts) pack, **CC0** — no attribution required, credited here anyway. The `x`/`y`/`g`/`escape` files were converted from the pack's SVG originals (source PNGs weren't at hand locally) at the same 64×64 as the rest — same shapes, same license, just re-rasterized.
- Only these 8 files are vendored, not the full ~5000-icon pack — the launcher only needs the four actions it actually has (`card_launch`, `card_add_to_steam`, `gallery_select`, `card_close_launcher`).

## App icon (`icon/`)

- `icon.png`, `icon.ico` — copied straight from `src-tauri/icons/` (Tatu's own icon), not a separate design. Same identity on the desktop tracker and the cartridge launcher.
