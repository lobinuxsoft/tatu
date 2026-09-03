class_name SteamShortcuts
extends RefCounted
## Builds and persists the GOG side of #209: a Steam Non-Steam shortcut per
## GOG app on the cartridge, plus whatever SteamGridDB art Tatu's HUB side
## already cached under assets/<app_id>/, applied via SteamCefClient. Kept
## out of main.gd — same reasoning as #208's VDF edit having its own file
## in steam_library.gd, one action flow per file.
##
## "Steam apps not owned by the destination account" (#209's other stated
## case) is deliberately NOT handled here: no verified SteamClient JS call
## for ownership exists in either reference project (CapyDeploy's own
## crates/steam/src/cef.rs or decky-capydeploy's eventPoller.tsx) — this
## is GOG-only until that's confirmed live.

## Sibling to the checksummed cartridge marker, never inside it — see
## marker.rs's own #209 warning about invalidating markers already in the
## wild by hashing a field that didn't exist when they were written.
const MAP_FILENAME := ".tatu-steam-shortcuts.json"

const ART_TYPES := {
	"grid": SteamCefClient.ASSET_GRID_LANDSCAPE,
	"hero": SteamCefClient.ASSET_HERO,
	"logo": SteamCefClient.ASSET_LOGO,
	"icon": SteamCefClient.ASSET_ICON,
}
const IMAGE_EXTENSIONS := ["png", "jpg", "jpeg", "webp"]

## app_id -> already-created Steam shortcut appid, read-only from Tatu's
## side, written only by this launcher.
static func load_map(cartridge_root: String) -> Dictionary:
	var path := cartridge_root.path_join(MAP_FILENAME)
	if not FileAccess.file_exists(path):
		return {}
	var parsed: Variant = JSON.parse_string(FileAccess.get_file_as_string(path))
	return parsed if typeof(parsed) == TYPE_DICTIONARY else {}

static func save_map(cartridge_root: String, map: Dictionary) -> void:
	var f := FileAccess.open(cartridge_root.path_join(MAP_FILENAME), FileAccess.WRITE)
	f.store_string(JSON.stringify(map))

## Applies shortcut + art for every GOG app on the cartridge not already
## tracked in the mapping file — idempotent across repeated "Add Cartridge"
## presses. One bad app (missing exe, failed AddShortcut) is skipped, never
## blocks the rest.
static func apply_gog_apps(client: SteamCefClient, cartridge_root: String, apps: Array) -> void:
	var map := load_map(cartridge_root)
	var changed := false
	for app in apps:
		var app_dict: Dictionary = app
		if String(app_dict.get("source", "steam")) != "gog":
			continue
		var app_id := int(app_dict.get("app_id", 0))
		if map.has(str(app_id)):
			continue
		var exe_relative := String(app_dict.get("exe_path", ""))
		if exe_relative.is_empty():
			continue

		var exe_path := cartridge_root.path_join(exe_relative)
		var steam_app_id := await client.add_shortcut(
			String(app_dict.get("name", "?")), exe_path, exe_path.get_base_dir()
		)
		if steam_app_id == 0:
			continue
		if exe_path.get_extension().to_lower() == "exe":
			await client.specify_compat_tool(steam_app_id, "proton_experimental")
		await _apply_art(client, cartridge_root, app_id, steam_app_id)

		map[str(app_id)] = steam_app_id
		changed = true
	if changed:
		save_map(cartridge_root, map)

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

## `assets/<app_id>/<type>.<ext>` — same layout main.gd's own
## `_grid_art_path` already reads for `grid`; hero/logo/icon aren't fetched
## by Tatu's HUB side yet (#209 follow-up), so those two just find nothing
## and get skipped until that pipeline exists.
static func _art_path(cartridge_root: String, app_id: int, art_type: String) -> String:
	var dir := cartridge_root.path_join("assets").path_join(str(app_id))
	for ext in IMAGE_EXTENSIONS:
		var candidate := dir.path_join("%s.%s" % [art_type, ext])
		if FileAccess.file_exists(candidate):
			return candidate
	return ""
