class_name SteamLibrary
## Self-contained `libraryfolders.vdf` reader/writer for #208 — deliberately
## NOT sharing code with Tatu's own Rust-side `steam::install` module: this
## has to work on a destination machine that never had Tatu installed,
## only whatever Steam client is already there.
##
## Steam owns and rewrites this file while running (same reason
## `config.vdf`/`localconfig.vdf` edits elsewhere in this epic require
## Steam closed first) — the caller (main.gd) is responsible for that,
## this module only ever transforms file content already in memory.

## Every already-registered `"path"` value, Windows `\\` separators
## normalized to `/` — matches the format the launcher already expects
## everywhere else in this project.
static func registered_paths(content: String) -> Array[String]:
	var paths: Array[String] = []
	var re := RegEx.new()
	re.compile("\"path\"\\s*\"([^\"]+)\"")
	for m in re.search_all(content):
		paths.append(m.get_string(1).replace("\\\\", "/"))
	return paths

## One past the highest top-level numeric key already in the file — Valve's
## own client always emits sequential "0", "1", "2"... entries; matching
## that instead of picking an arbitrary free number keeps the file looking
## like Steam wrote it itself.
static func _next_index(content: String) -> int:
	var re := RegEx.new()
	re.compile("\"(\\d+)\"\\s*\\{")
	var highest := -1
	for m in re.search_all(content):
		highest = maxi(highest, int(m.get_string(1)))
	return highest + 1

## Appends a minimal new library entry for `path`, right before the file's
## last closing brace — a no-op if `path` is already registered. Steam
## fills in `contentid`/`totalsize`/`apps` itself on its next launch,
## verified live once already this epic: a minimal entry survives and gets
## corrected, never rejected.
static func add_library(content: String, path: String) -> String:
	if registered_paths(content).has(path):
		return content
	var index := _next_index(content)
	var entry := "\t\"%d\"\n\t{\n\t\t\"path\"\t\t\"%s\"\n\t}\n" % [index, path]
	var last_brace := content.rfind("}")
	if last_brace == -1:
		return content
	return content.substr(0, last_brace) + entry + content.substr(last_brace)
