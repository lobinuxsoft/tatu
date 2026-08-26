class_name GameCard
extends Button
## One card in the launcher's carousel (#204) — a portrait cover-art tile,
## rounded corners + drop shadow, matching Steam's own library-capsule
## proportions rather than a landscape thumbnail.
##
## Design adapted from ShadowBlip/OpenGamepadUI's card.tscn (GPL-3.0,
## compatible with this project's AGPL-3.0) — a real, actively maintained,
## gamepad-native Godot 4 launcher solving this exact screen. Three ideas
## borrowed directly: portrait aspect ratio with STRETCH_KEEP_ASPECT_COVERED
## (art always fills the tile, no letterbox gaps), a name label that only
## exists as a bottom overlay for a game with no cached cover art (the art
## itself is the identity when one exists — no separate label to keep
## aligned across cards), and a StyleBoxFlat drop shadow behind the panel.
##
## Because the label lives INSIDE the fixed-aspect panel instead of below
## it, scaling the whole card on selection (Control.scale) is safe here —
## an earlier version scaled a card with an EXTERNAL label underneath and
## the two drifted apart differently per card (see PR #211's history).

signal clicked(index: int)

## width / height — Steam's own library-capsule proportion. The carousel
## (main.gd) decides the actual pixel size from the window's own height, so
## this is only ever used as a ratio, never a fixed pixel constant.
const ASPECT_RATIO := 220.0 / 330.0
const NAME_OVERLAY_HEIGHT_RATIO := 56.0 / 330.0
const CORNER_RADIUS := 18
const SELECTED_SCALE := Vector2(1.08, 1.08)
const SELECT_DURATION := 0.15

var index: int = 0
var _panel: PanelContainer
var _art: TextureRect
var _name_overlay: PanelContainer
var _name_label: Label
var _tween: Tween

func _init() -> void:
	flat = true
	focus_mode = Control.FOCUS_NONE

	# Drop shadow, behind everything, same footprint as the card.
	var shadow_style := StyleBoxFlat.new()
	shadow_style.bg_color = Color(0, 0, 0, 0)
	shadow_style.set_corner_radius_all(CORNER_RADIUS)
	shadow_style.shadow_color = Color(0, 0, 0, 0.45)
	shadow_style.shadow_size = 16
	shadow_style.shadow_offset = Vector2(0, 6)
	var shadow := PanelContainer.new()
	shadow.show_behind_parent = true
	shadow.mouse_filter = Control.MOUSE_FILTER_IGNORE
	shadow.set_anchors_preset(Control.PRESET_FULL_RECT)
	shadow.add_theme_stylebox_override("panel", shadow_style)
	add_child(shadow)

	var panel_style := StyleBoxFlat.new()
	panel_style.bg_color = Color(0.16, 0.16, 0.18)
	panel_style.set_corner_radius_all(CORNER_RADIUS)

	_panel = PanelContainer.new()
	_panel.clip_contents = true
	_panel.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_panel.set_anchors_preset(Control.PRESET_FULL_RECT)
	_panel.add_theme_stylebox_override("panel", panel_style)
	add_child(_panel)

	_art = TextureRect.new()
	_art.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	_art.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_COVERED
	_art.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_panel.add_child(_art)

	var overlay_style := StyleBoxFlat.new()
	overlay_style.bg_color = Color(0.1, 0.1, 0.12, 0.75)
	overlay_style.content_margin_left = 10
	overlay_style.content_margin_right = 10
	overlay_style.content_margin_top = 8
	overlay_style.content_margin_bottom = 8
	_name_overlay = PanelContainer.new()
	_name_overlay.visible = false
	_name_overlay.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_name_overlay.set_anchors_preset(Control.PRESET_BOTTOM_WIDE)
	_name_overlay.add_theme_stylebox_override("panel", overlay_style)
	_art.add_child(_name_overlay)

	_name_label = Label.new()
	_name_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_name_label.autowrap_mode = TextServer.AUTOWRAP_WORD
	_name_label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_name_overlay.add_child(_name_label)

func _ready() -> void:
	pressed.connect(func() -> void: clicked.emit(index))

func setup(i: int, display_name: String, art_path: String) -> void:
	index = i
	_name_label.text = display_name
	_load_art(art_path)

## Sizes this card off a target HEIGHT, deriving width from ASPECT_RATIO —
## called by the carousel whenever the window/carousel area is resized, so
## cards scale with the screen instead of staying a fixed pixel size.
func resize(height: float) -> void:
	var new_size := Vector2(height * ASPECT_RATIO, height)
	custom_minimum_size = new_size
	size = new_size
	pivot_offset = new_size / 2.0
	# PRESET_BOTTOM_WIDE (set once in _init) pins top+bottom anchors to the
	# same ratio, so its actual height only ever comes from this offset.
	_name_overlay.offset_top = -height * NAME_OVERLAY_HEIGHT_RATIO

## Called by the carousel every time the selection changes — this card does
## not track its own selected state.
func set_selected(selected: bool) -> void:
	var target_scale := SELECTED_SCALE if selected else Vector2.ONE
	var target_modulate := Color.WHITE if selected else Color(1, 1, 1, 0.6)
	if _tween:
		_tween.kill()
	_tween = create_tween().set_parallel(true)
	_tween.tween_property(self, "scale", target_scale, SELECT_DURATION) \
		.set_trans(Tween.TRANS_BACK).set_ease(Tween.EASE_OUT)
	_tween.tween_property(self, "modulate", target_modulate, SELECT_DURATION) \
		.set_trans(Tween.TRANS_SINE).set_ease(Tween.EASE_OUT)

func _load_art(path: String) -> void:
	if path.is_empty():
		_name_overlay.visible = true
		return
	var image := Image.new()
	var err := image.load(path)
	if err != OK:
		push_warning("Cannot load cover art at %s: error %d" % [path, err])
		_name_overlay.visible = true
		return
	_art.texture = ImageTexture.create_from_image(image)
