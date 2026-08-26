extends Control
## Cartridge launcher entry point (#204). Reads #193's marker directly — no
## separate manifest — and renders one animated card per installed app.
## Never re-verifies the marker's checksum: yaguarete_os#314 already does
## that before autorunning this binary, so redoing it here would just
## duplicate that trust boundary for no benefit.

const MARKER_FILENAME := ".tatu-cartridge.json"
const GRID_ART_EXTENSIONS: Array[String] = ["png", "jpg", "jpeg", "webp"]
const GRID_COLUMNS := 4

var _grid: GridContainer
var _empty_state: Label

func _ready() -> void:
	_build_layout()
	var apps := _load_apps()
	_empty_state.visible = apps.is_empty()
	_grid.get_parent().visible = not apps.is_empty()
	for app: Dictionary in apps:
		_add_card(app)

func _build_layout() -> void:
	_empty_state = Label.new()
	_empty_state.text = "No hay juegos instalados en este cartucho."
	_empty_state.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_empty_state.set_anchors_preset(Control.PRESET_CENTER)
	add_child(_empty_state)

	var scroll := ScrollContainer.new()
	scroll.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(scroll)

	_grid = GridContainer.new()
	_grid.columns = GRID_COLUMNS
	scroll.add_child(_grid)

func _cartridge_root() -> String:
	return OS.get_executable_path().get_base_dir()

func _load_apps() -> Array:
	var marker_path := _cartridge_root().path_join(MARKER_FILENAME)
	if not FileAccess.file_exists(marker_path):
		push_warning("No cartridge marker at %s" % marker_path)
		return []

	var parsed: Variant = JSON.parse_string(FileAccess.get_file_as_string(marker_path))
	if typeof(parsed) != TYPE_DICTIONARY or not parsed.has("apps"):
		push_warning("Cartridge marker at %s is not valid JSON" % marker_path)
		return []

	return parsed["apps"]

func _add_card(app: Dictionary) -> void:
	var app_id := int(app.get("app_id", 0))
	var card := GameCard.new()
	_grid.add_child(card)
	card.setup(app_id, String(app.get("name", "?")), _grid_art_path(app_id))
	card.launch_requested.connect(_on_launch_requested)

func _grid_art_path(app_id: int) -> String:
	var dir := _cartridge_root().path_join("assets").path_join(str(app_id))
	for ext in GRID_ART_EXTENSIONS:
		var candidate := dir.path_join("grid.%s" % ext)
		if FileAccess.file_exists(candidate):
			return candidate
	return ""

func _on_launch_requested(app_id: int) -> void:
	# Execution paths (#206 Linux, #207 Windows) own how a game actually
	# launches — this issue only owns the UI and the manifest read.
	push_warning("Launch requested for app %d — no execution path wired yet (#206/#207)" % app_id)
