extends Control
## Cartridge launcher entry point (#204). Reads #193's marker directly — no
## separate manifest — and renders a Steam-Deck-style carousel: the selected
## app always sits centered and enlarged, with an info panel on one side and
## the two direct actions (launch standalone / add to Steam) on the other.
## Only two actions ever exist, so they are bound straight to a gamepad
## button + a keyboard key rather than needing on-screen buttons to
## navigate to.
##
## Never re-verifies the marker's checksum: yaguarete_os#314 already does
## that before autorunning this binary, so redoing it here would just
## duplicate that trust boundary for no benefit.

const MARKER_FILENAME := ".tatu-cartridge.json"
const GRID_ART_EXTENSIONS: Array[String] = ["png", "jpg", "jpeg", "webp"]
const CARD_SPACING := 240.0
const MOVE_DURATION := 0.18
const SIDE_PANEL_WIDTH := 300
const SIDE_PANEL_MARGIN := 24

var _apps: Array = []
var _selected_index: int = 0
var _cards: Array[GameCard] = []

var _empty_state: Label
var _carousel_clip: Control
var _carousel_track: Control
var _info_name: Label
var _info_description: Label
var _action_launch: Label
var _action_add_to_steam: Label

func _ready() -> void:
	_register_input_actions()
	_build_layout()
	_apps = _load_apps()
	_empty_state.visible = _apps.is_empty()
	_carousel_clip.visible = not _apps.is_empty()
	for i in _apps.size():
		_add_card(i, _apps[i])
	if _apps.is_empty():
		return
	# Containers (the HBoxContainer split below) only settle sizes one idle
	# frame after their children change — reading _carousel_clip.size any
	# earlier would compute the carousel's center against a stale size.
	await get_tree().process_frame
	_update_selection()

func _register_input_actions() -> void:
	_ensure_action("card_launch", KEY_ENTER, JOY_BUTTON_A)
	_ensure_action("card_add_to_steam", KEY_S, JOY_BUTTON_B)

func _ensure_action(action: StringName, key: int, joy_button: int) -> void:
	if InputMap.has_action(action):
		return
	InputMap.add_action(action)
	var key_event := InputEventKey.new()
	key_event.keycode = key
	InputMap.action_add_event(action, key_event)
	var joy_event := InputEventJoypadButton.new()
	joy_event.button_index = joy_button
	InputMap.action_add_event(action, joy_event)

func _unhandled_input(event: InputEvent) -> void:
	if _apps.is_empty():
		return
	if event.is_action_pressed("ui_left"):
		_move_selection(-1)
	elif event.is_action_pressed("ui_right"):
		_move_selection(1)
	elif event.is_action_pressed("card_launch"):
		_on_launch_requested()
	elif event.is_action_pressed("card_add_to_steam"):
		_on_add_to_steam_requested()

func _move_selection(delta: int) -> void:
	_selected_index = wrapi(_selected_index + delta, 0, _apps.size())
	_update_selection()

func _build_layout() -> void:
	var root := HBoxContainer.new()
	root.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(root)

	_info_name = Label.new()
	_info_name.add_theme_font_size_override("font_size", 22)
	_info_description = Label.new()
	_info_description.autowrap_mode = TextServer.AUTOWRAP_WORD
	root.add_child(_side_panel([_info_name, _info_description]))

	_carousel_clip = Control.new()
	_carousel_clip.clip_contents = true
	_carousel_clip.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_carousel_clip.size_flags_vertical = Control.SIZE_EXPAND_FILL
	root.add_child(_carousel_clip)

	_carousel_track = Control.new()
	_carousel_track.set_anchors_preset(Control.PRESET_FULL_RECT)
	_carousel_clip.add_child(_carousel_track)

	_action_launch = Label.new()
	_action_launch.text = "[A] / Enter — Ejecutar sin Steam"
	_action_launch.autowrap_mode = TextServer.AUTOWRAP_WORD
	_action_add_to_steam = Label.new()
	_action_add_to_steam.text = "[B] / S — Agregar a Steam"
	_action_add_to_steam.autowrap_mode = TextServer.AUTOWRAP_WORD
	root.add_child(_side_panel([_action_launch, _action_add_to_steam]))

	_empty_state = Label.new()
	_empty_state.text = "No hay juegos instalados en este cartucho."
	_empty_state.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_empty_state.set_anchors_preset(Control.PRESET_CENTER)
	add_child(_empty_state)

func _side_panel(children: Array) -> Control:
	var margin := MarginContainer.new()
	margin.custom_minimum_size = Vector2(SIDE_PANEL_WIDTH, 0)
	margin.size_flags_vertical = Control.SIZE_EXPAND_FILL
	for side in ["left", "top", "right", "bottom"]:
		margin.add_theme_constant_override("margin_%s" % side, SIDE_PANEL_MARGIN)

	var panel := VBoxContainer.new()
	panel.size_flags_vertical = Control.SIZE_EXPAND_FILL
	panel.alignment = BoxContainer.ALIGNMENT_CENTER
	panel.add_theme_constant_override("separation", 12)
	for child: Control in children:
		panel.add_child(child)

	margin.add_child(panel)
	return margin

func _cartridge_root() -> String:
	# Hitting Play in the editor runs the Godot editor binary itself, which
	# never has a real cartridge next to it — the empty state would be the
	# ONLY reachable outcome otherwise. test_cartridge/ is a fixture for
	# iterating on the carousel; an exported build never takes this branch,
	# OS.has_feature("editor") is false there.
	if OS.has_feature("editor"):
		return ProjectSettings.globalize_path("res://test_cartridge")
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

func _add_card(i: int, app: Dictionary) -> void:
	var app_id := int(app.get("app_id", 0))
	var card := GameCard.new()
	_carousel_track.add_child(card)
	card.setup(i, String(app.get("name", "?")), _grid_art_path(app_id))
	card.clicked.connect(_on_card_clicked)
	_cards.append(card)

func _on_card_clicked(index: int) -> void:
	_selected_index = index
	_update_selection()

func _grid_art_path(app_id: int) -> String:
	var dir := _cartridge_root().path_join("assets").path_join(str(app_id))
	for ext in GRID_ART_EXTENSIONS:
		var candidate := dir.path_join("grid.%s" % ext)
		if FileAccess.file_exists(candidate):
			return candidate
	return ""

func _description_of(app_id: int) -> String:
	var path := _cartridge_root().path_join("assets").path_join(str(app_id)).path_join("description.txt")
	if not FileAccess.file_exists(path):
		return ""
	return FileAccess.get_file_as_string(path)

func _update_selection() -> void:
	if _apps.is_empty():
		return

	var center := _carousel_track.size.x / 2.0
	for i in _cards.size():
		var card := _cards[i]
		var offset := i - _selected_index
		var target_x := center + offset * CARD_SPACING - card.custom_minimum_size.x / 2.0
		var target_y := (_carousel_track.size.y - card.custom_minimum_size.y) / 2.0
		var tween := create_tween()
		tween.tween_property(card, "position", Vector2(target_x, target_y), MOVE_DURATION)
		card.set_selected(i == _selected_index)

	var app: Dictionary = _apps[_selected_index]
	var app_id := int(app.get("app_id", 0))
	_info_name.text = String(app.get("name", "?"))
	_info_description.text = _description_of(app_id)

	var standalone := bool(app.get("standalone", false))
	_action_launch.modulate = Color.WHITE if standalone else Color(1, 1, 1, 0.4)

func _on_launch_requested() -> void:
	var app_id := int(_apps[_selected_index].get("app_id", 0))
	var standalone := bool(_apps[_selected_index].get("standalone", false))
	if not standalone:
		push_warning("App %d needs Steam — #206/#207 only cover standalone launches" % app_id)
		return
	# Execution paths (#206 Linux, #207 Windows) own how a game actually
	# launches — this issue only owns the UI and the manifest read.
	push_warning("Launch requested for app %d — no execution path wired yet (#206/#207)" % app_id)

func _on_add_to_steam_requested() -> void:
	var app_id := int(_apps[_selected_index].get("app_id", 0))
	# Steam library registration (#208) is not wired yet.
	push_warning("Add-to-Steam requested for app %d — no registration wired yet (#208)" % app_id)
