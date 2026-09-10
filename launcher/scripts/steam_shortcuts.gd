class_name SteamShortcuts
extends RefCounted
## Builds and persists a Steam Non-Steam shortcut for every GOG (#209) or
## non-Steam (#236/#329) app on the cartridge — neither ever gets a real
## Steam library entry from `_launch_via_steam`'s own `libraryfolders.vdf`
## edit, since neither was ever a Steam-owned install to begin with. Plus
## whatever SteamGridDB art Tatu's HUB side already cached under
## assets/<app_id>/, applied via SteamCefClient. Kept out of main.gd — same
## reasoning as #208's VDF edit having its own file in steam_library.gd,
## one action flow per file.
##
## "Steam apps not owned by the destination account" (#209's other stated
## case) is deliberately NOT handled here: no verified SteamClient JS call
## for ownership exists in either reference project (CapyDeploy's own
## crates/steam/src/cef.rs or decky-capydeploy's eventPoller.tsx).

## Lives under Godot's own per-machine `user://` data dir, NEVER on the
## cartridge — a Steam shortcut (`shortcuts.vdf`) is inherently local to
## whichever machine's Steam install created it, but the cartridge itself
## travels between machines. Confirmed live: a shortcut created on one PC
## left this map showing the app as "already done," then the SAME cartridge
## on a second machine (with no shortcut of its own — `shortcuts.vdf` never
## existed there) skipped it entirely on "Add Cartridge", silently creating
## nothing. Keying only by `app_id` (not also by cartridge/machine) is fine:
## each machine now has its own file, so there's nothing left to collide.
const MAP_FILENAME := "user://steam_shortcuts.json"

## Sources that need a Steam shortcut created for them — a real Steam app
## already gets a library entry for free from `_launch_via_steam`'s own
## `libraryfolders.vdf` edit, so it's deliberately absent here.
const SHORTCUT_SOURCES := ["gog", "non_steam"]

## `grid`/`grid_landscape` mirror `save_selected_artwork_sync`'s own Rust-side
## slot names (artwork_search.rs) — `grid` is always the PORTRAIT pick
## (`ArtworkSelection::grid_portrait`), `grid_landscape` the wide one. An
## earlier version of this table mapped `grid` to `ASSET_GRID_LANDSCAPE`
## and never read `grid_landscape` at all, silently swapping the two
## orientations in Steam and dropping the landscape pick entirely.
##
## `icon`/`ASSET_ICON` is deliberately ABSENT — confirmed live with two
## throwaway 1x1 PNGs sent back-to-back: `SetCustomArtworkForApp(id, data,
## "png", ASSET_GRID_LANDSCAPE)` immediately followed by `(..., ASSET_ICON)`
## left only the ICON one on disk, in the LANDSCAPE slot — Steam's own
## artwork setter doesn't have a real 5th (icon) destination, asset type 4
## just clobbers type 3's file. decky-capydeploy's own reference never
## calls this for icon either — it writes the icon file directly into
## `config/grid/<id>_icon.<ext>` and patches `shortcuts.vdf`'s icon field
## by hand instead, a different mechanism entirely (#329 follow-up, not
## implemented here). Sending it through this call is strictly worse than
## not sending it — it doesn't set an icon AND it corrupts landscape.
const ART_TYPES := {
	"grid": SteamCefClient.ASSET_GRID_PORTRAIT,
	"grid_landscape": SteamCefClient.ASSET_GRID_LANDSCAPE,
	"hero": SteamCefClient.ASSET_HERO,
	"logo": SteamCefClient.ASSET_LOGO,
}
const IMAGE_EXTENSIONS := ["png", "jpg", "jpeg", "webp"]

## app_id -> already-created Steam shortcut appid on THIS machine, read-only
## from Tatu's side, written only by this launcher.
static func load_map() -> Dictionary:
	if not FileAccess.file_exists(MAP_FILENAME):
		return {}
	var parsed: Variant = JSON.parse_string(FileAccess.get_file_as_string(MAP_FILENAME))
	return parsed if typeof(parsed) == TYPE_DICTIONARY else {}

static func save_map(map: Dictionary) -> void:
	var f := FileAccess.open(MAP_FILENAME, FileAccess.WRITE)
	f.store_string(JSON.stringify(map))

## Applies shortcut + art for every GOG/non-Steam app on the cartridge not
## already tracked in the mapping file — idempotent across repeated "Add
## Cartridge" presses. One bad app (missing exe, failed AddShortcut) is
## skipped, never blocks the rest.
##
## `resolve_exe` is `main.gd::_resolved_exe_path` bound as a Callable
## `(exe_relative, source, app_id) -> String` — same lookup "Launch" already
## uses to prefer a local/library copy over the cartridge one (including a
## #335 user-chosen destination), so a shortcut created AFTER "Copiar a
## carpeta local"/"...de Steam" finished points at that copy instead of the
## cartridge (live-reported: the copy finished but Steam had no shortcut
## for it at all yet — the fix is this call reusing the exact same
## resolution "Launch" always did, not a second copy step).
static func apply_shortcuts(
	client: SteamCefClient, cartridge_root: String, apps: Array, resolve_exe: Callable
) -> void:
	var map := load_map()
	var changed := false
	for app in apps:
		var app_dict: Dictionary = app
		if String(app_dict.get("source", "steam")) not in SHORTCUT_SOURCES:
			continue
		var app_id := int(app_dict.get("app_id", 0))
		if map.has(str(app_id)):
			continue
		var app_name := String(app_dict.get("name", "?"))
		var exe_relative := String(app_dict.get("exe_path", ""))
		if exe_relative.is_empty():
			push_warning("Steam shortcut skipped for \"%s\": no exe_path on the marker" % app_name)
			continue

		var source := String(app_dict.get("source", "steam"))
		var exe_path: String = resolve_exe.call(exe_relative, source, app_id)
		var steam_app_id := await client.add_shortcut(
			app_name, exe_path, exe_path.get_base_dir()
		)
		if steam_app_id == 0:
			# `add_shortcut` already pushed the specific CDP failure (no debug
			# tabs, exception, timeout) via `_evaluate` — this just marks
			# which app that failure belonged to, since the loop otherwise
			# swallows it with no way to tell which of several apps failed.
			push_warning("Steam shortcut failed for \"%s\": AddShortcut returned 0" % app_name)
			continue
		if exe_path.get_extension().to_lower() == "exe":
			# Confirmed live: calling SpecifyCompatTool immediately after the
			# AddShortcut that just created THIS app_id left no compat tool
			# mapping at all (config.vdf's own CompatToolMapping never got
			# the entry, despite the CDP call itself reporting success, no
			# exception) — the exact same call against an app_id that had
			# already existed for a while worked instantly. Same race class
			# already assumed for set_custom_artwork's own Clear→sleep→Set.
			await (Engine.get_main_loop() as SceneTree).create_timer(1.0).timeout
			await client.specify_compat_tool(steam_app_id, "proton_experimental")
		await _apply_art(client, cartridge_root, app_id, steam_app_id)

		map[str(app_id)] = steam_app_id
		changed = true
	if changed:
		save_map(map)

static func _apply_art(
	client: SteamCefClient, cartridge_root: String, app_id: int, steam_app_id: int
) -> void:
	for art_type in ART_TYPES:
		var art_path := _art_path(cartridge_root, app_id, art_type)
		if art_path.is_empty():
			continue
		var bytes := FileAccess.get_file_as_bytes(art_path)
		await client.set_custom_artwork(
			steam_app_id, Marshalls.raw_to_base64(bytes), ART_TYPES[art_type]
		)

## `assets/<app_id>/<type>.<ext>` — the RAW pick Tatu's HUB side cached
## (artwork_search.rs's own `grid`/`grid_landscape`/`hero`/`logo` slots),
## deliberately NOT `main.gd::_grid_art_path`'s `card.<ext>` — that one is a
## static-only substitute for THIS launcher's own card, useless (and
## sometimes even absent) for Steam's shortcut art, which can render an
## animated pick natively. GOG's auto-pick only ever writes `grid`, so the
## other three just find nothing and get skipped for those apps.
static func _art_path(cartridge_root: String, app_id: int, art_type: String) -> String:
	var dir := cartridge_root.path_join("assets").path_join(str(app_id))
	for ext in IMAGE_EXTENSIONS:
		var candidate := dir.path_join("%s.%s" % [art_type, ext])
		if FileAccess.file_exists(candidate):
			return candidate
	return ""
