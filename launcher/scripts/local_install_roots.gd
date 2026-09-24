class_name LocalInstallRoots
extends RefCounted
## Per-machine record of a user-chosen destination for a GOG/non-Steam
## local copy (#335) — Godot's own `user://` data dir, NEVER the cartridge.
## A chosen filesystem path only means something on THIS machine; the
## cartridge travels between machines (same lesson SteamShortcuts' own
## tracking map already learned the hard way, #333).
##
## Real Steam apps don't need an equivalent of this: `libraryfolders.vdf`
## is already Steam's own per-machine registry of every library it knows
## about, so `_resolved_exe_path` just scans all of those directly instead
## of tracking a second, redundant map for the same information.

const MAP_FILENAME := "user://local_install_roots.json"

static func load_map() -> Dictionary:
	if not FileAccess.file_exists(MAP_FILENAME):
		return {}
	var parsed: Variant = JSON.parse_string(FileAccess.get_file_as_string(MAP_FILENAME))
	return parsed if typeof(parsed) == TYPE_DICTIONARY else {}

static func save_map(map: Dictionary) -> void:
	var f := FileAccess.open(MAP_FILENAME, FileAccess.WRITE)
	f.store_string(JSON.stringify(map))

## `default_root` (Tatu's own `_tatu_local_dir()`) is what every previous
## copy already used — apps copied before this feature existed have no
## entry here at all, and must keep resolving to exactly where they
## actually are.
static func get_root(app_id: int, default_root: String) -> String:
	var map := load_map()
	return String(map.get(str(app_id), default_root))

static func set_root(app_id: int, root: String) -> void:
	var map := load_map()
	map[str(app_id)] = root
	save_map(map)
