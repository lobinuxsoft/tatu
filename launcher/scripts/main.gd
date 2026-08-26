extends Control
## Cartridge launcher entry point (#204). Reads #193's marker directly — no
## separate manifest — and renders a Steam-Deck-style carousel: the selected
## app always sits centered and enlarged, with an info panel on one side and
## the two direct actions (launch standalone / add to Steam) on the other.
## Only two actions ever exist, so they are bound straight to a gamepad
## button + a keyboard key rather than needing on-screen buttons to
## navigate to.
##
## The carousel is a real HBoxContainer of cards (gets spacing/sizing right
## for free — an earlier hand-rolled version positioned each card by hand
## and had cards bleeding past the side panels, see PR #211's history)
## sitting inside a plain clipped Control. A ScrollContainer was tried in
## between: it cannot scroll a card to center-screen when the total row is
## NARROWER than the viewport (a small cartridge with few games) — its
## scroll_horizontal clamps to 0, so nothing short of overflowing content
## can ever be centered. Animating the ROW's own position instead has no
## such floor. Studied ShadowBlip/OpenGamepadUI (GPL-3.0, compatible with
## this project's AGPL-3.0) before rewriting this — a real, maintained,
## gamepad-native Godot launcher building the exact same kind of screen.
##
## Never re-verifies the marker's checksum: yaguarete_os#314 already does
## that before autorunning this binary, so redoing it here would just
## duplicate that trust boundary for no benefit.

const MARKER_FILENAME := ".tatu-cartridge.json"
const GRID_ART_EXTENSIONS: Array[String] = ["png", "jpg", "jpeg", "webp"]
# Cards size off the carousel area's own height, not a fixed pixel value —
# resizing the window (or running on a different screen entirely) has to
# rescale them, not leave them stuck at whatever size they started at.
const CARD_HEIGHT_RATIO := 0.62
const CARD_GAP_RATIO := 0.06
const SCROLL_DURATION := 0.4
# Every size below is a RATIO of the window's own size, never a fixed pixel
# constant — a first pass used fixed pixels throughout (300px panels,
# 22px/16px fonts) and it read fine on the 1152x648 debug window but broke
# down completely at a real screen size: tiny text in a huge empty panel,
# or (once fonts scaled up to fix that) a panel too narrow to fit them.
# Everything has to scale off the same window together or they drift apart.
const SIDE_PANEL_WIDTH_RATIO := 0.22
const SIDE_PANEL_MARGIN_RATIO := 0.018
const TITLE_FONT_RATIO := 0.036
const BODY_FONT_RATIO := 0.02
const HINT_ICON_RATIO := 0.036

# Same display/body fonts Tatu's own web frontend themes already use (OFL,
# vendored under assets/fonts/ — see assets/README.md for provenance).
const FONT_DISPLAY := "res://assets/fonts/Rajdhani-Bold.ttf"
const FONT_BODY := "res://assets/fonts/Inter-Regular.ttf"
# Kenney's CC0 Input Prompts pack (assets/README.md) — Steam Deck face
# buttons, since that's the physical device this whole look targets.
const ICON_LAUNCH: Array[String] = [
	"res://assets/input_prompts/keyboard_enter.png",
	"res://assets/input_prompts/steamdeck_button_a.png",
]
const ICON_ADD_TO_STEAM: Array[String] = [
	"res://assets/input_prompts/keyboard_s.png",
	"res://assets/input_prompts/steamdeck_button_b.png",
]

var _apps: Array = []
var _selected_index: int = 0
var _cards: Array[GameCard] = []

var _empty_state: Label
var _background: TextureRect
var _carousel_clip: Control
var _carousel_row: HBoxContainer
var _info_name: Label
var _info_description: Label
var _action_launch: Control
var _action_add_to_steam: Control
var _scroll_tween: Tween

var _left_glass: ColorRect
var _left_margin: MarginContainer
var _right_glass: ColorRect
var _right_margin: MarginContainer

# Everything whose size scales with the window — populated as each is
# built, resized together in _resize_layout().
var _title_labels: Array[Label] = []
var _body_labels: Array[Label] = []
var _hint_icons: Array[TextureRect] = []

var _dragging := false
var _drag_start_mouse_x := 0.0
var _drag_start_row_x := 0.0

func _ready() -> void:
	_register_input_actions()
	_build_layout()
	_carousel_clip.resized.connect(_on_carousel_resized)
	_apps = _load_apps()
	_empty_state.visible = _apps.is_empty()
	_carousel_clip.visible = not _apps.is_empty()
	for i in _apps.size():
		_add_card(i, _apps[i])
	if _apps.is_empty():
		return
	# The HBoxContainer only settles each card's real position one idle
	# frame after they're all added — centering the scroll any earlier
	# would compute against a stale (zero) position for the first card.
	await get_tree().process_frame
	_resize_layout()
	_update_selection(false)

## Re-sizes cards, fonts, and hint icons off the carousel area's CURRENT
## height and re-centers without animating — called on the initial layout
## and again every time the window (or just this area) is resized.
func _resize_layout() -> void:
	var title_size := int(_carousel_clip.size.y * TITLE_FONT_RATIO)
	for label in _title_labels:
		label.add_theme_font_size_override("font_size", title_size)

	var body_size := int(_carousel_clip.size.y * BODY_FONT_RATIO)
	for label in _body_labels:
		label.add_theme_font_size_override("font_size", body_size)

	var icon_size := _carousel_clip.size.y * HINT_ICON_RATIO
	for icon in _hint_icons:
		icon.custom_minimum_size = Vector2(icon_size, icon_size)

	var panel_width := _carousel_clip.size.x * SIDE_PANEL_WIDTH_RATIO
	var panel_margin := int(_carousel_clip.size.x * SIDE_PANEL_MARGIN_RATIO)
	_left_glass.offset_right = panel_width
	_right_glass.offset_left = -panel_width
	for side in ["left", "top", "right", "bottom"]:
		_left_margin.add_theme_constant_override("margin_%s" % side, panel_margin)
		_right_margin.add_theme_constant_override("margin_%s" % side, panel_margin)

	if _cards.is_empty():
		return
	var card_height := _carousel_clip.size.y * CARD_HEIGHT_RATIO
	_carousel_row.add_theme_constant_override("separation", int(card_height * CARD_GAP_RATIO))
	for card in _cards:
		card.resize(card_height)

func _on_carousel_resized() -> void:
	_resize_layout()
	if _apps.is_empty():
		return
	_center_on_selected(false)

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
	_update_selection(true)

func _build_layout() -> void:
	# Full-screen backdrop: the selected app's own cover art, blown up and
	# blurred, first child so everything else draws on top of it.
	_background = TextureRect.new()
	_background.set_anchors_preset(Control.PRESET_FULL_RECT)
	_background.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	_background.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_COVERED
	_background.modulate = Color(0.55, 0.55, 0.6)
	_background.material = ShaderMaterial.new()
	_background.material.shader = load("res://shaders/box_blur.gdshader")
	add_child(_background)

	# The carousel spans the FULL screen — the glass side panels overlay on
	# top of its edges rather than living in separate side-by-side columns,
	# so a card visibly slides (and softens through the glass shader) behind
	# them instead of stopping short at a hard-clipped gap.
	_carousel_clip = Control.new()
	_carousel_clip.clip_contents = true
	_carousel_clip.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(_carousel_clip)

	_carousel_row = HBoxContainer.new()
	_carousel_clip.add_child(_carousel_row)
	# Click-and-drag to browse — a quick click still reaches each GameCard's
	# own `pressed` first (its mouse_filter is PASS, not the default STOP,
	# specifically so this signal still fires for a press that started on a
	# card), this only sees the drag itself.
	_carousel_clip.gui_input.connect(_on_carousel_gui_input)

	_info_name = Label.new()
	_info_name.add_theme_font_override("font", load(FONT_DISPLAY))
	_title_labels.append(_info_name)
	_info_description = Label.new()
	_info_description.add_theme_font_override("font", load(FONT_BODY))
	_info_description.autowrap_mode = TextServer.AUTOWRAP_WORD
	_body_labels.append(_info_description)
	add_child(_glass_panel(Control.PRESET_LEFT_WIDE, [_info_name, _info_description]))

	_action_launch = _action_hint(ICON_LAUNCH, "Ejecutar sin Steam")
	_action_add_to_steam = _action_hint(ICON_ADD_TO_STEAM, "Agregar a Steam")
	add_child(_glass_panel(Control.PRESET_RIGHT_WIDE, [_action_launch, _action_add_to_steam]))

	_empty_state = Label.new()
	_empty_state.text = "No hay juegos instalados en este cartucho."
	_empty_state.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_empty_state.set_anchors_preset(Control.PRESET_CENTER)
	add_child(_empty_state)

## A strip pinned to the given edge (PRESET_LEFT_WIDE or PRESET_RIGHT_WIDE),
## rendering whatever is already on screen behind it — blurred and tinted —
## via glass_panel.gdshader, added AFTER the carousel so that shader's
## SCREEN_TEXTURE capture actually includes the cards. Width and margin are
## set once here and rewritten by _resize_layout() on every resize.
func _glass_panel(preset: LayoutPreset, children: Array) -> Control:
	var glass := ColorRect.new()
	glass.set_anchors_preset(preset)
	glass.material = ShaderMaterial.new()
	glass.material.shader = load("res://shaders/glass_panel.gdshader")

	var margin := MarginContainer.new()
	margin.set_anchors_preset(Control.PRESET_FULL_RECT)
	glass.add_child(margin)

	if preset == Control.PRESET_LEFT_WIDE:
		_left_glass = glass
		_left_margin = margin
	else:
		_right_glass = glass
		_right_margin = margin

	var panel := VBoxContainer.new()
	panel.size_flags_vertical = Control.SIZE_EXPAND_FILL
	panel.alignment = BoxContainer.ALIGNMENT_CENTER
	panel.add_theme_constant_override("separation", 18)
	for child: Control in children:
		panel.add_child(child)

	margin.add_child(panel)
	return glass

## A row of input-prompt icons (keyboard key + gamepad button) followed by
## the action's label — real icons instead of a "[A] / Enter —" text hint.
func _action_hint(icon_paths: Array[String], text: String) -> Control:
	var row := HBoxContainer.new()
	row.alignment = BoxContainer.ALIGNMENT_CENTER
	row.add_theme_constant_override("separation", 8)
	for path in icon_paths:
		var icon := TextureRect.new()
		icon.texture = load(path)
		icon.custom_minimum_size = Vector2(28, 28)
		icon.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
		row.add_child(icon)
		_hint_icons.append(icon)
	var label := Label.new()
	label.text = text
	label.add_theme_font_override("font", load(FONT_BODY))
	label.autowrap_mode = TextServer.AUTOWRAP_WORD
	label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_body_labels.append(label)
	row.add_child(label)
	return row

func _on_carousel_gui_input(event: InputEvent) -> void:
	if event is InputEventMouseButton and event.button_index == MOUSE_BUTTON_LEFT:
		if event.pressed:
			_dragging = true
			_drag_start_mouse_x = event.position.x
			_drag_start_row_x = _carousel_row.position.x
			if _scroll_tween:
				_scroll_tween.kill()
		elif _dragging:
			_dragging = false
			_snap_to_nearest_card()
	elif _dragging and event is InputEventMouseMotion:
		var motion := event as InputEventMouseMotion
		var delta := motion.position.x - _drag_start_mouse_x
		_carousel_row.position.x = _drag_start_row_x + delta

## Whichever card ends up closest to the viewport's center when the drag is
## released becomes the new selection — the same centering tween in
## _update_selection then finishes the motion from wherever the drag left it.
func _snap_to_nearest_card() -> void:
	var viewport_center := _carousel_clip.size.x / 2.0
	var best_index := _selected_index
	var best_distance := INF
	for i in _cards.size():
		var card_center := _carousel_row.position.x + _cards[i].position.x + _cards[i].size.x / 2.0
		var distance := absf(card_center - viewport_center)
		if distance < best_distance:
			best_distance = distance
			best_index = i
	_selected_index = best_index
	_update_selection(true)

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
	_carousel_row.add_child(card)
	card.setup(i, String(app.get("name", "?")), _grid_art_path(app_id))
	card.clicked.connect(_on_card_clicked)
	_cards.append(card)

func _on_card_clicked(index: int) -> void:
	_selected_index = index
	_update_selection(true)

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

func _update_selection(animate: bool) -> void:
	if _apps.is_empty():
		return

	for i in _cards.size():
		_cards[i].set_selected(i == _selected_index)
	_center_on_selected(animate)

	var app: Dictionary = _apps[_selected_index]
	var app_id := int(app.get("app_id", 0))
	_info_name.text = String(app.get("name", "?"))
	_info_description.text = _description_of(app_id)
	_update_background(app_id)

	var standalone := bool(app.get("standalone", false))
	_action_launch.modulate = Color.WHITE if standalone else Color(1, 1, 1, 0.4)

## The blurred backdrop reuses the same cover art the selected card already
## shows — no separate asset, no separate cache.
func _update_background(app_id: int) -> void:
	var path := _grid_art_path(app_id)
	if path.is_empty():
		_background.texture = null
		return
	var image := Image.new()
	if image.load(path) != OK:
		_background.texture = null
		return
	_background.texture = ImageTexture.create_from_image(image)

## Moves the whole row so the selected card's center lands on the clip
## area's center. The row's OWN sizing/spacing comes for free from being a
## real HBoxContainer — this only ever touches the row's outer position,
## never an individual card's.
func _center_on_selected(animate: bool) -> void:
	var card := _cards[_selected_index]
	var target_x := _carousel_clip.size.x / 2.0 - (card.position.x + card.size.x / 2.0)
	var target_y := (_carousel_clip.size.y - _carousel_row.size.y) / 2.0
	var target := Vector2(target_x, target_y)
	if _scroll_tween:
		_scroll_tween.kill()
	if not animate:
		_carousel_row.position = target
		return
	_scroll_tween = create_tween()
	_scroll_tween.tween_property(_carousel_row, "position", target, SCROLL_DURATION) \
		.set_trans(Tween.TRANS_BACK).set_ease(Tween.EASE_OUT)

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
