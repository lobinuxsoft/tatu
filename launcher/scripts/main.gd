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
const IMAGE_EXTENSIONS: Array[String] = ["png", "jpg", "jpeg", "webp"]
# Opt-in trailer (#212), Ogg Theora — the only video format Godot 4 plays
# natively. Absent for most games unless the user checked "Incluir
# trailers" in the Cartucho tab: falls back to a cached screenshot, then
# the blurred grid art, same tier order the background already had.
const TRAILER_FILENAME := "trailer.ogv"
# Cards size off the carousel area's own height, not a fixed pixel value —
# resizing the window (or running on a different screen entirely) has to
# rescale them, not leave them stuck at whatever size they started at.
# Reduced from 0.62 (2026-08-31, user request): a smaller cover leaves more
# of the blurred background trailer/screenshot actually visible around it.
const CARD_HEIGHT_RATIO := 0.48
const CARD_GAP_RATIO := 0.06
# Where the row sits vertically within the carousel clip area — 0.5 is dead
# center, 1.0 would rest its bottom edge on the clip's own bottom edge.
# Biased down (2026-08-31, user request) so more of the background
# trailer/screenshot shows above the cards instead of being split evenly.
const CARD_VERTICAL_BIAS_RATIO := 0.68
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
# Smaller and its own ratio, not BODY_FONT_RATIO — these sit in their own
# bottom-center bar (see ACTION_BAR_* below), so shrinking them doesn't
# fight the info panel's actual body text for space. Reduced again from an
# earlier pass — found live-testing with the user: all four still need to
# fit in one row without crowding.
const HINT_FONT_RATIO := 0.011
const HINT_ICON_RATIO := 0.02
# The action bar's own bottom margin and the gap between its four action
# groups, both ratios of the window rather than fixed pixels — same rule
# as every other size in this file.
# Live-reported (2026-08-31): a visible gap sat between the glass bar and
# the actual bottom edge of the screen — flush against the edge reads more
# like a real console/Deck button-prompt bar.
const ACTION_BAR_MARGIN_RATIO := 0.0
const ACTION_BAR_GAP_RATIO := 0.03
# Height of the frosted-glass strip behind the action bar, and the padding
# between that strip's edge and the icons/labels inside it — the bar had no
# background at all before this (live-reported, 2026-08-31: icons/text
# vanished against a bright background image). Same shader the side panels
# already use (`glass_panel.gdshader`), just a bottom-anchored strip instead
# of a full-height side one.
const ACTION_BAR_GLASS_HEIGHT_RATIO := 0.07
const ACTION_BAR_GLASS_PADDING_RATIO := 0.012

# Same display/body fonts Tatu's own web frontend themes already use (OFL,
# vendored under assets/fonts/ — see assets/README.md for provenance).
const FONT_DISPLAY := "res://assets/fonts/Rajdhani-Bold.ttf"
const FONT_BODY := "res://assets/fonts/Inter-Regular.ttf"
# Kenney's CC0 Input Prompts pack (assets/README.md) — Steam Deck face
# buttons, since that's the physical device this whole look targets. A, X,
# Y, B map to launch/add-to-steam/gallery/close, in that order — matches
# the layout of the buttons themselves on a real controller, not an
# arbitrary pick.
const ICON_LAUNCH: Array[String] = [
	"res://assets/input_prompts/keyboard_enter.png",
	"res://assets/input_prompts/steamdeck_button_a.png",
]
const ICON_ADD_TO_STEAM: Array[String] = [
	"res://assets/input_prompts/keyboard_s.png",
	"res://assets/input_prompts/steamdeck_button_x.png",
]
const ICON_GALLERY: Array[String] = [
	"res://assets/input_prompts/keyboard_g.png",
	"res://assets/input_prompts/steamdeck_button_y.png",
]
const ICON_CLOSE: Array[String] = [
	"res://assets/input_prompts/keyboard_escape.png",
	"res://assets/input_prompts/steamdeck_button_b.png",
]

# #206: umu-run + Proton + Steam Linux Runtime, bundled onto the cartridge
# by Tatu's runtime.rs at the same moment Goldberg injection runs — must
# match those filenames/pins exactly, or the launcher extracts nothing.
const CARTRIDGE_RUNTIME_SUBDIR := "runtime/linux"
const RUNTIME_ARCHIVE := "SteamLinuxRuntime_4.tar.xz"
const PROTON_ARCHIVE := "GE-Proton11-5-x86_64.tar.gz"
const PROTON_DIRNAME := "GE-Proton11-5-x86_64"

# #209: GOG-shortcut side, applied via CDP once Steam is back up after
# `_launch_via_steam`'s own restart below.
const CEF_DEBUG_FILE := ".cef-enable-remote-debugging"
const CEF_WAIT_TIMEOUT_SEC := 30.0
const CEF_POLL_INTERVAL_SEC := 2.0

var _apps: Array = []
var _selected_index: int = 0
var _cards: Array[GameCard] = []

# Cover art decodes are spread across frames instead of blocking the first
# one for as long as the whole library's art takes to read off the
# cartridge (#249) — each entry is {"card": GameCard, "path": String}.
var _pending_art: Array[Dictionary] = []
const ART_LOAD_BUDGET_MS := 4.0

var _empty_state: Label
var _background: TextureRect
var _background_video: VideoStreamPlayer
var _background_loading: Label
# Bumped on every _update_background call — a background-thread load whose
# id no longer matches when it finishes belongs to a selection the player
# already scrolled past, and must not overwrite what's shown now (#254).
var _background_load_id := 0
var _carousel_clip: Control
var _carousel_row: HBoxContainer
var _info_name: Label
var _info_description: Label
var _action_launch: Control
var _action_add_to_steam: Control
var _action_gallery: Control
var _action_close: Control
var _action_bar: HBoxContainer
var _gallery: ScreenshotGallery
var _viewer: Control
var _viewer_image: TextureRect
var _action_status: Label
var _action_overlay: Control
var _scroll_tween: Tween
var _panel_content_width := 0.0

var _left_glass: ColorRect
var _left_margin: MarginContainer
var _right_glass: ColorRect
var _right_margin: MarginContainer
var _action_bar_glass: ColorRect
var _action_bar_margin: MarginContainer

# Everything whose size scales with the window — populated as each is
# built, resized together in _resize_layout().
var _title_labels: Array[Label] = []
var _body_labels: Array[Label] = []
var _hint_labels: Array[Label] = []
var _hint_icons: Array[TextureRect] = []

var _dragging := false
var _drag_start_mouse_x := 0.0
var _drag_start_row_x := 0.0

func _ready() -> void:
	# `window/size/mode=3` (borderless fullscreen) isn't enough on its own on
	# every compositor — some never hand a newly-mapped fullscreen window
	# input focus, leaving the gamepad dead until the user clicks it by hand.
	get_window().grab_focus()
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
	# resize_layout() sets custom_minimum_size/size directly on every card,
	# which only marks the HBoxContainer dirty — it re-sorts children on the
	# NEXT idle frame, not synchronously. Centering right here would read
	# every card's PRE-resize position, landing the carousel offset wrong
	# on every single launch (confirmed live, #247).
	await get_tree().process_frame
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

	var hint_size := int(_carousel_clip.size.y * HINT_FONT_RATIO)
	for label in _hint_labels:
		label.add_theme_font_size_override("font_size", hint_size)

	var icon_size := _carousel_clip.size.y * HINT_ICON_RATIO
	for icon in _hint_icons:
		icon.custom_minimum_size = Vector2(icon_size, icon_size)

	var bar_margin := int(_carousel_clip.size.y * ACTION_BAR_MARGIN_RATIO)
	var bar_height := int(_carousel_clip.size.y * ACTION_BAR_GLASS_HEIGHT_RATIO)
	var bar_padding := int(_carousel_clip.size.y * ACTION_BAR_GLASS_PADDING_RATIO)
	_action_bar_glass.offset_bottom = -bar_margin
	_action_bar_glass.offset_top = -bar_margin - bar_height
	for side in ["left", "top", "right", "bottom"]:
		_action_bar_margin.add_theme_constant_override("margin_%s" % side, bar_padding)
	_action_bar.add_theme_constant_override(
		"separation", int(_carousel_clip.size.x * ACTION_BAR_GAP_RATIO)
	)

	var panel_width := _carousel_clip.size.x * SIDE_PANEL_WIDTH_RATIO
	var panel_margin := int(_carousel_clip.size.x * SIDE_PANEL_MARGIN_RATIO)
	_left_glass.offset_right = panel_width
	_right_glass.offset_left = -panel_width
	for side in ["left", "top", "right", "bottom"]:
		_left_margin.add_theme_constant_override("margin_%s" % side, panel_margin)
		_right_margin.add_theme_constant_override("margin_%s" % side, panel_margin)
	_panel_content_width = panel_width - panel_margin * 2.0
	_gallery.resize(_panel_content_width)

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
	# Same one-frame container-sort lag as _ready() above.
	await get_tree().process_frame
	_center_on_selected(false)

func _register_input_actions() -> void:
	_ensure_action("card_launch", KEY_ENTER, JOY_BUTTON_A)
	_ensure_action("card_add_to_steam", KEY_S, JOY_BUTTON_X)
	# ui_up/ui_down are built-in (arrow keys + D-pad/stick already wired by
	# Godot's default input map) — the screenshot gallery reuses them
	# directly rather than registering its own, same as the carousel
	# already reuses ui_left/ui_right instead of custom actions.
	_ensure_action("gallery_select", KEY_G, JOY_BUTTON_Y)
	_ensure_action("gallery_close", KEY_ESCAPE, JOY_BUTTON_B)
	# Same physical keys as gallery_close above, on purpose: the two are
	# checked in mutually exclusive branches below (viewer open vs. not),
	# so ESCAPE/B reads as one consistent "back" button either way — closes
	# whatever's in front, up to and including the launcher itself.
	_ensure_action("card_close_launcher", KEY_ESCAPE, JOY_BUTTON_B)

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
	# The enlarged viewer is a modal — while it's open, it owns input
	# exclusively (no carousel navigation leaking through behind it). #214's
	# bonus-content gallery reuses this exact same trap. ui_up/ui_down still
	# move through the screenshots while enlarged, though — found live-
	# testing with the user: the viewer opened but there was no way to
	# browse past the one screenshot you enlarged first.
	if _viewer.visible:
		if event.is_action_pressed("gallery_close") or event.is_action_pressed("gallery_select"):
			_close_viewer()
		elif event.is_action_pressed("ui_up"):
			_gallery.move_selection(-1)
			_open_viewer(_gallery.selected_path())
		elif event.is_action_pressed("ui_down"):
			_gallery.move_selection(1)
			_open_viewer(_gallery.selected_path())
		return
	if _apps.is_empty():
		return
	if event.is_action_pressed("ui_left"):
		_move_selection(-1)
	elif event.is_action_pressed("ui_right"):
		_move_selection(1)
	elif event.is_action_pressed("ui_up"):
		_gallery.move_selection(-1)
	elif event.is_action_pressed("ui_down"):
		_gallery.move_selection(1)
	elif event.is_action_pressed("gallery_select"):
		_open_viewer(_gallery.selected_path())
	elif event.is_action_pressed("card_launch"):
		_on_launch_requested()
	elif event.is_action_pressed("card_add_to_steam"):
		_on_add_to_steam_requested()
	elif event.is_action_pressed("card_close_launcher"):
		get_tree().quit()

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

	# Same full-rect slot as the TextureRect above — only one of the two is
	# ever visible at a time (#212), toggled in _update_background(). Muted:
	# this is background dressing behind the carousel, not something that
	# should compete with whatever audio the player already has going.
	_background_video = VideoStreamPlayer.new()
	_background_video.set_anchors_preset(Control.PRESET_FULL_RECT)
	_background_video.expand = true
	_background_video.loop = true
	_background_video.volume_db = -80.0
	_background_video.visible = false
	add_child(_background_video)


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

	_gallery = ScreenshotGallery.new()
	_gallery.thumbnail_activated.connect(_open_viewer)
	add_child(_glass_panel(Control.PRESET_RIGHT_WIDE, [_gallery]))

	# Bottom-center action bar, horizontal — found live-testing with the
	# user: stacked vertically inside the side panel, the four action rows
	# crowded the gallery above them and read too large/prominent. A single
	# row across the bottom of the whole screen (not just the side panel)
	# reads more like a real game's button-prompt bar, and leaves the
	# gallery its full height to work with.
	_action_launch = _action_hint(ICON_LAUNCH, "Launch")
	_action_add_to_steam = _action_hint(ICON_ADD_TO_STEAM, "Add Cartridge")
	_action_gallery = _action_hint(ICON_GALLERY, "Gallery")
	_action_close = _action_hint(ICON_CLOSE, "Close")
	_action_bar = HBoxContainer.new()
	_action_bar.alignment = BoxContainer.ALIGNMENT_CENTER
	for action in [_action_launch, _action_add_to_steam, _action_gallery, _action_close]:
		_action_bar.add_child(action)

	# Frosted-glass background, same shader + structure the side panels
	# already use (glass ColorRect -> MarginContainer -> content), just a
	# bottom-anchored strip instead of a full-height side one. Live-reported
	# (2026-08-31): the bar had no background at all before this, and its
	# white icons/text vanished against a bright background image.
	_action_bar_glass = ColorRect.new()
	_action_bar_glass.set_anchors_preset(Control.PRESET_BOTTOM_WIDE)
	_action_bar_glass.grow_vertical = Control.GROW_DIRECTION_BEGIN
	_action_bar_glass.material = ShaderMaterial.new()
	_action_bar_glass.material.shader = load("res://shaders/glass_panel.gdshader")
	# Added before the screenshot viewer and the launch overlay below, both
	# of which cover the whole screen — without a z_index they'd draw over
	# this bar instead of under it, hiding the input prompts (live-tested).
	_action_bar_glass.z_index = 100
	_action_bar_margin = MarginContainer.new()
	_action_bar_margin.set_anchors_preset(Control.PRESET_FULL_RECT)
	# Centers the row inside the glass strip on BOTH axes — a MarginContainer
	# alone only insets the child by the margins below, it doesn't center it
	# if the strip ends up taller than the row's own natural height.
	var bar_center := CenterContainer.new()
	bar_center.add_child(_action_bar)
	_action_bar_margin.add_child(bar_center)
	_action_bar_glass.add_child(_action_bar_margin)
	add_child(_action_bar_glass)

	_empty_state = Label.new()
	_empty_state.text = "No hay juegos instalados en este cartucho."
	_empty_state.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_empty_state.set_anchors_preset(Control.PRESET_CENTER)
	add_child(_empty_state)

	# Full-screen modal, built last so it draws on top of everything else —
	# hidden until a screenshot is activated.
	_viewer = ColorRect.new()
	_viewer.color = Color(0, 0, 0, 0.85)
	_viewer.set_anchors_preset(Control.PRESET_FULL_RECT)
	_viewer.visible = false
	_viewer_image = TextureRect.new()
	_viewer_image.set_anchors_preset(Control.PRESET_FULL_RECT)
	_viewer_image.offset_left = 80
	_viewer_image.offset_top = 80
	_viewer_image.offset_right = -80
	_viewer_image.offset_bottom = -80
	_viewer_image.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	_viewer.add_child(_viewer_image)
	add_child(_viewer)

	# Launching (#206) shells out and waits on `_ensure_linux_runtime_deployed`
	# extracting ~700MB the first time — with zero UI feedback that used to
	# look identical to the launcher having hung. Built last, same as the
	# viewer above, so it draws on top of everything.
	_action_overlay = ColorRect.new()
	_action_overlay.color = Color(0, 0, 0, 0.75)
	_action_overlay.set_anchors_preset(Control.PRESET_FULL_RECT)
	_action_overlay.visible = false
	_action_status = Label.new()
	_action_status.add_theme_font_override("font", load(FONT_DISPLAY))
	_action_status.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_action_status.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	_action_status.set_anchors_preset(Control.PRESET_FULL_RECT)
	_title_labels.append(_action_status)
	_action_overlay.add_child(_action_status)
	add_child(_action_overlay)

	# Shown while _update_background's read is still going — a slow USB/NTFS
	# drive made switching selection feel frozen with no feedback at all
	# (#254). Built here, not next to the background TextureRect/VideoPlayer
	# above: an earlier version added it right after those and it rendered
	# invisible, hidden behind the carousel and side panels drawn later in
	# the tree (#258) — Control siblings paint back-to-front by add order.
	_background_loading = Label.new()
	_background_loading.text = "Cargando..."
	_background_loading.set_anchors_preset(Control.PRESET_CENTER)
	_background_loading.add_theme_font_override("font", load(FONT_BODY))
	_background_loading.visible = false
	add_child(_background_loading)

	add_child(_build_info_label())

## Small, dim corner label showing exactly which build is on screen —
## `<version>+g<commit>[-dirty]`, written by `launcher/export.sh` into
## `build_info.txt` right before each export. Added after repeatedly
## confusing a stale cached binary for a just-fixed one during live
## testing with no way to tell them apart on screen. Missing file (a
## debug run straight from the editor, not an exported build) just shows
## nothing rather than an error — this is diagnostic, never load-bearing.
func _build_info_label() -> Label:
	var label := Label.new()
	label.set_anchors_preset(Control.PRESET_BOTTOM_RIGHT)
	label.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	label.grow_vertical = Control.GROW_DIRECTION_BEGIN
	label.offset_left = -160
	label.offset_top = -24
	label.modulate = Color(1, 1, 1, 0.35)
	label.add_theme_font_size_override("font_size", 12)
	var path := "res://build_info.txt"
	label.text = FileAccess.get_file_as_string(path).strip_edges() if FileAccess.file_exists(path) else ""
	return label

## A strip pinned to the given edge (PRESET_LEFT_WIDE or PRESET_RIGHT_WIDE),
## rendering whatever is already on screen behind it — blurred and tinted —
## via glass_panel.gdshader, added AFTER the carousel so that shader's
## SCREEN_TEXTURE capture actually includes the cards. Width and margin are
## set once here and rewritten by _resize_layout() on every resize.
func _glass_panel(preset: LayoutPreset, children: Array, vbox_alignment := BoxContainer.ALIGNMENT_CENTER) -> Control:
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
	panel.alignment = vbox_alignment
	panel.add_theme_constant_override("separation", 12)
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
	# No autowrap/EXPAND_FILL here — this used to be a wide row inside the
	# vertical side-panel stack, but now sits as one of four fixed-size
	# groups in the horizontal bottom bar. Wrapping mid-word ("Add to
	# Steam" over 3 lines) was the visible result of the old assumption.
	_hint_labels.append(label)
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
	# Dev-only escape hatch: point the editor-flavored binary at a REAL
	# cartridge mount point (e.g. `--cartridge-root=/run/media/.../CART`)
	# instead of the fixture below — lets #206/#207's execution paths be
	# smoke-tested against real Steam data without needing an exported
	# build + export templates just to flip OS.has_feature("editor") off.
	# Never reachable on a real exported binary (no matching arg exists).
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--cartridge-root="):
			return arg.trim_prefix("--cartridge-root=")

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
	card.setup(i, String(app.get("name", "?")))
	card.clicked.connect(_on_card_clicked)
	_cards.append(card)
	_pending_art.append({"card": card, "path": _grid_art_path(app_id)})

## Decodes queued cover art a few milliseconds at a time instead of all at
## once — the first card (already selected/visible) is always the first
## one processed, so the one art the user sees immediately never waits
## behind the rest of the library.
func _process(_delta: float) -> void:
	if _pending_art.is_empty():
		set_process(false)
		return
	var deadline := Time.get_ticks_msec() + ART_LOAD_BUDGET_MS
	while not _pending_art.is_empty() and Time.get_ticks_msec() < deadline:
		var item: Dictionary = _pending_art.pop_front()
		(item["card"] as GameCard).load_art(item["path"])

func _on_card_clicked(index: int) -> void:
	_selected_index = index
	_update_selection(true)

func _grid_art_path(app_id: int) -> String:
	var dir := _cartridge_root().path_join("assets").path_join(str(app_id))
	for ext in IMAGE_EXTENSIONS:
		var candidate := dir.path_join("grid.%s" % ext)
		if FileAccess.file_exists(candidate):
			return candidate
	return ""

## #213's screenshots (Tatu-side pipeline not built yet — this reads
## whatever the fixture/cartridge already has under screenshots/, same as
## _grid_art_path already does for grid.*). Sorted by filename so the
## gallery order is stable across relaunches.
func _screenshot_paths(app_id: int) -> Array[String]:
	var paths: Array[String] = []
	var dir_path := _cartridge_root().path_join("assets").path_join(str(app_id)).path_join("screenshots")
	var dir := DirAccess.open(dir_path)
	if dir == null:
		return paths
	dir.list_dir_begin()
	var file_name := dir.get_next()
	while file_name != "":
		if not dir.current_is_dir() and file_name.get_extension().to_lower() in IMAGE_EXTENSIONS:
			paths.append(dir_path.path_join(file_name))
		file_name = dir.get_next()
	dir.list_dir_end()
	paths.sort()
	return paths

func _open_viewer(path: String) -> void:
	if path.is_empty():
		return
	var image := Image.new()
	if image.load(path) != OK:
		push_warning("Cannot load screenshot at %s" % path)
		return
	_viewer_image.texture = ImageTexture.create_from_image(image)
	_viewer.visible = true

func _close_viewer() -> void:
	_viewer.visible = false

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
	var screenshots := _screenshot_paths(app_id)
	_info_name.text = String(app.get("name", "?"))
	_info_description.text = _description_of(app_id)
	_update_background(app_id, screenshots)
	_gallery.set_screenshots(screenshots)
	_gallery.resize(_panel_content_width)

	var standalone := bool(app.get("standalone", false))
	_action_launch.modulate = Color.WHITE if standalone else Color(1, 1, 1, 0.4)

## Background priority (#212): a cached trailer beats a cached screenshot
## beats the blurred grid art — closer to actual gameplay each step down,
## and no tier ever leaves the background blank since grid art (#205) is
## already required for the card itself to render at all.
##
## The actual disk read runs on a worker thread (#254) — a slow USB/NTFS
## cartridge made every selection change stall the whole UI while a
## screenshot or trailer read off it, with zero feedback that anything was
## happening.
##
## An earlier version moved the actual read to a WorkerThreadPool task —
## reverted (2026-08-29, #256) after it crashed live with "double free or
## corruption": a worker thread calling back into this same script
## instance's methods raced the main thread running _process()/input
## handling on that same instance, corrupting GDScript's own heap. Back to
## fully synchronous; `await get_tree().process_frame` just buys the
## "Cargando..." label one real frame to draw before the blocking read
## starts, which is the only part of this that's actually safe to do
## without a thread.
func _update_background(app_id: int, screenshots: Array[String]) -> void:
	_background_load_id += 1
	var request_id := _background_load_id
	_background_loading.visible = true
	await get_tree().process_frame
	if request_id != _background_load_id:
		return

	var trailer_path := _trailer_path(app_id)
	if not trailer_path.is_empty():
		var stream := VideoStreamTheora.new()
		stream.set_file(trailer_path)
		_background_video.stream = stream
		_background_video.play()
		_background_video.visible = true
		_background.visible = false
		_background_loading.visible = false
		return

	_background_video.stop()
	_background_video.visible = false
	_background.visible = true

	var path := screenshots[0] if not screenshots.is_empty() else _grid_art_path(app_id)
	if path.is_empty():
		_background.texture = null
		_background_loading.visible = false
		return
	var image := Image.new()
	if image.load(path) != OK:
		_background.texture = null
		_background_loading.visible = false
		return
	_background.texture = ImageTexture.create_from_image(image)
	_background_loading.visible = false

func _trailer_path(app_id: int) -> String:
	var path := _cartridge_root().path_join("assets").path_join(str(app_id)).path_join(TRAILER_FILENAME)
	return path if FileAccess.file_exists(path) else ""

## Moves the whole row so the selected card's center lands on the clip
## area's center. The row's OWN sizing/spacing comes for free from being a
## real HBoxContainer — this only ever touches the row's outer position,
## never an individual card's.
func _center_on_selected(animate: bool) -> void:
	var card := _cards[_selected_index]
	var target_x := _carousel_clip.size.x / 2.0 - (card.position.x + card.size.x / 2.0)
	var target_y := (_carousel_clip.size.y - _carousel_row.size.y) * CARD_VERTICAL_BIAS_RATIO
	var target := Vector2(target_x, target_y)
	if _scroll_tween:
		_scroll_tween.kill()
	if not animate:
		_carousel_row.position = target
		return
	# TRANS_CUBIC instead of TRANS_BACK (user request, 2026-08-31): TRANS_BACK
	# overshoots past the target and springs back — smooth deceleration reads
	# calmer for a row of cards sliding into place.
	_scroll_tween = create_tween()
	_scroll_tween.tween_property(_carousel_row, "position", target, SCROLL_DURATION) \
		.set_trans(Tween.TRANS_CUBIC).set_ease(Tween.EASE_OUT)

func _on_launch_requested() -> void:
	var app: Dictionary = _apps[_selected_index]
	var app_id := int(app.get("app_id", 0))
	var app_name := String(app.get("name", "el juego"))

	# Hard/Unknown preservability (real third-party DRM, or not enough data
	# to tell) never gets Goldberg's standalone patch (#199) — it only ever
	# plays through a real Steam client, registered first via the dedicated
	# "Add Cartridge" action (#208) below, not this one.
	if not bool(app.get("standalone", false)):
		await _show_status("Usá \"Add Cartridge\" para jugar %s" % app_name, 2.5)
		return

	var exe_relative := String(app.get("exe_path", ""))
	if exe_relative.is_empty():
		push_warning("App %d has no exe_path on the marker — Goldberg injection (#199) never ran" % app_id)
		return
	var exe_path := _cartridge_root().path_join(exe_relative)

	if OS.get_name() != "Linux":
		push_warning("Standalone launch on %s not wired yet (#207)" % OS.get_name())
		return
	await _launch_via_proton(app_id, app_name, exe_path)

## Shows the action-status overlay with `text` for `seconds`, then hides it —
## shared by the launch flow below and by Add Cartridge, so every action a
## player takes gets SOME visible response instead of only a `push_warning`
## that only ever reached a log file nobody was watching.
func _show_status(text: String, seconds: float) -> void:
	_action_status.text = text
	_action_overlay.visible = true
	await get_tree().create_timer(seconds).timeout
	_action_overlay.visible = false

## Single-quotes `s` for `/bin/sh -c`, escaping any embedded single quote —
## paths here come from the marker/cartridge layout, not untrusted input,
## but "Alabaster Dawn"-style spaces make quoting mandatory regardless.
func _sh_quote(s: String) -> String:
	return "'" + s.replace("'", "'\\''") + "'"

## Runs a Goldberg-patched exe through umu-run (#206) — adopted rather than
## hand-rolling a Proton invocation: it replicates Steam's own runtime
## container so the game behaves the same as it would through Steam,
## without needing Steam installed. See runtime.rs on the Tatu side for why
## this specific tool and the exact files it bundles onto the cartridge.
func _launch_via_proton(app_id: int, app_name: String, exe_path: String) -> void:
	_action_status.text = "Lanzando %s..." % app_name
	_action_overlay.visible = true
	# The first launch on a cartridge extracts ~700MB synchronously below —
	# without yielding a couple frames first, the overlay's own visibility
	# change would never actually get drawn before that freezes the thread,
	# so this would look exactly like the hang it's meant to explain away.
	await get_tree().process_frame
	await get_tree().process_frame

	if not _ensure_linux_runtime_deployed():
		await _show_status("No se pudo preparar el runtime de Linux para %s" % app_name, 2.5)
		return

	var wineprefix := _tatu_local_dir().path_join("wineprefix").path_join(str(app_id))
	DirAccess.make_dir_recursive_absolute(wineprefix)

	OS.set_environment("GAMEID", "umu-default")
	OS.set_environment("STORE", "none")
	OS.set_environment("PROTONPATH", _umu_compat_dir().path_join(PROTON_DIRNAME))
	OS.set_environment("WINEPREFIX", wineprefix)
	# The whole point of bundling the runtime on the cartridge is that the
	# destination machine never needs network access — this stops umu-run
	# from trying to check for a newer Steam Linux Runtime on its own.
	OS.set_environment("UMU_RUNTIME_UPDATE", "0")
	# Isolates umu-run's own storage under Tatu's folder instead of the
	# shared ~/.local/share/umu convention — a destination machine may
	# already run Lutris/Heroic with a real umu install there, and this
	# launcher has no business mixing its bundled runtime into it.
	OS.set_environment("UMU_FOLDERS_PATH", _tatu_local_dir())

	# `create_process` has no working_directory parameter (Godot 4.7's OS
	# class never grew one) — without this, umu-run and the game inherit
	# this launcher's own CWD instead of the game's install folder. Steam
	# itself always launches with CWD = install dir, and Goldberg's
	# steam_appid.txt lookup falls back to exactly that path when it's not
	# under steam_settings/, so a wrong CWD here silently breaks standalone
	# Goldberg games without touching a single one of their files.
	var install_dir := exe_path.get_base_dir()
	var umu_run := _tatu_local_dir().path_join("umu-run")
	var cmd := "cd %s && exec %s %s" % [_sh_quote(install_dir), _sh_quote(umu_run), _sh_quote(exe_path)]
	var pid := OS.create_process("/bin/sh", ["-c", cmd])
	if pid <= 0:
		push_warning("Failed to launch app %d via umu-run" % app_id)
		await _show_status("No se pudo lanzar %s" % app_name, 2.5)
		return

	# create_process is non-blocking — it returns as soon as umu-run starts,
	# long before Proton actually puts a game window on screen. Without
	# quitting here, this launcher keeps running underneath, still listening
	# for input: a stray Enter/gamepad-A press spawns ANOTHER copy of the
	# same game, indefinitely, real bug found live-testing with the user.
	# Bowing out entirely also frees the GPU for the game instead of two
	# graphical apps fighting over it — never a good idea on the handheld
	# hardware this cartridge is meant to run on.
	await get_tree().create_timer(1.5).timeout
	get_tree().quit()

## Registers the CARTRIDGE ITSELF as a Steam library folder (#208) — not any
## one game on it. Steam then scans the whole folder on its own and
## recognizes every app manifest already there, so this only ever needs to
## run once per cartridge no matter how many games get installed onto it
## afterward. Closes Steam if it's running (it owns `libraryfolders.vdf` in
## memory and would overwrite an edit made while it's still open, same
## reason `config.vdf`/`localconfig.vdf` edits elsewhere in this epic
## require it closed), writes a minimal entry to both copies of the file,
## then reopens it.
##
## ALWAYS runs the full stop/edit/restart cycle, even if the path is already
## in `libraryfolders.vdf` — Steam periodically re-validates removable-media
## libraries on its own and can mark one "not mounted" in memory (confirmed
## live in this machine's own `content_log.txt`, taking down an unrelated
## real external library at the same moment, not something this code
## caused) without ever rewriting the file to say so. A file-only "already
## registered" check can't tell that apart from a genuinely healthy one, so
## it's not trustworthy here — only a real restart reliably fixes it, and
## this is a deliberate, infrequent, user-triggered action, not something
## that runs on its own.
##
## Deliberately does NOT also fire `steam://rungameid/<id>` to auto-launch
## anything — Steam's own startup time is unpredictable enough (can run
## into the tens of seconds on a cold start) that firing it right after
## reopening would be a guess, not something verified to actually work.
## Getting every installed game to appear, ready to press Play like any
## other title in the user's library, is what #208 actually asks for.
func _launch_via_steam() -> void:
	var mount_point := _cartridge_root()
	var steam_dir := _steam_install_dir()
	if steam_dir.is_empty():
		await _show_status("No se encontró una instalación de Steam en esta máquina", 2.5)
		return

	_action_status.text = "Registrando el cartucho en Steam..."
	_action_overlay.visible = true
	await get_tree().process_frame
	await get_tree().process_frame

	await _stop_steam()

	# A PC that never ran Tatu still auto-mounts this cartridge through
	# udisks2, so the fix can't live only in Tatu's UI — it has to travel
	# with the launcher. After `_stop_steam` so the drive isn't busy when
	# this remounts it (see `_ensure_ntfs_symlinks`'s own header).
	if OS.get_name() != "Windows":
		await _ensure_ntfs_symlinks()

	for rel in ["config/libraryfolders.vdf", "steamapps/libraryfolders.vdf"]:
		var path := steam_dir.path_join(rel)
		if not FileAccess.file_exists(path):
			continue
		var content := FileAccess.get_file_as_string(path)
		var updated: String = load("res://scripts/steam_library.gd").add_library(content, mount_point)
		if updated == content:
			continue
		var f := FileAccess.open(path, FileAccess.WRITE)
		f.store_string(updated)
		f.close()

	_start_steam(steam_dir)
	await _apply_gog_steam_shortcuts()
	# Live-tested (2026-08-28): Steam's own shader-cache download+commit for a
	# freshly-launched app on a USB cartridge took ~10 minutes and stalled the
	# client's main thread the whole time — looks exactly like a hang. Only
	# fires the first time Steam runs this app on this machine/prefix, but
	# this overlay is gone by then (launcher already quit), so the warning
	# has to land here, before that.
	_action_status.text = "Abriendo Steam... el primer lanzamiento de cada juego puede tardar varios minutos (shader cache) — no lo cierres aunque parezca colgado."
	await get_tree().create_timer(4.0).timeout
	get_tree().quit()

## #209: GOG games never show up in a real Steam library no matter how many
## folders get scanned — Steam only recognizes ITS OWN manifests there. A
## Non-Steam shortcut is the only way in, applied via CDP against the same
## Steam instance `_launch_via_steam` just restarted above. A no-op when
## the cartridge has no GOG apps at all.
func _apply_gog_steam_shortcuts() -> void:
	if not _apps.any(func(a): return String(a.get("source", "steam")) == "gog"):
		return
	_ensure_cef_debug_file()
	_action_status.text = "Configurando accesos directos de GOG en Steam..."
	if not await _wait_for_cef():
		push_warning("Steam CEF debug port never came up — GOG shortcuts skipped (#209)")
		return
	await SteamShortcuts.apply_gog_apps(SteamCefClient.new(), _cartridge_root(), _apps)

## Mirrors CapyDeploy's controller.rs::ensure_cef_debug_file — an empty
## sentinel Steam checks for at startup before opening its CDP debug port.
## Best-effort: a machine that's never had this file needs Steam restarted
## once before the port comes up, which `_launch_via_steam` is already
## doing right above every call site of this function.
func _ensure_cef_debug_file() -> void:
	var path := _steam_install_dir().path_join(CEF_DEBUG_FILE)
	if FileAccess.file_exists(path):
		return
	var f := FileAccess.open(path, FileAccess.WRITE)
	if f:
		f.close()

func _wait_for_cef() -> bool:
	var deadline := Time.get_ticks_msec() + int(CEF_WAIT_TIMEOUT_SEC * 1000)
	while Time.get_ticks_msec() < deadline:
		if SteamCefClient.is_available():
			return true
		await get_tree().create_timer(CEF_POLL_INTERVAL_SEC).timeout
	return false

## udisks2 forces `windows_names` onto every NTFS drive it automounts by
## default (storaged-project/udisks#620) — it forbids the `:` in Wine's own
## `dosdevices/c:` symlink, so Proton's own prefix creation fails with
## EINVAL. Confirmed live on the desktop app (`cartridge/symlinks.rs`);
## reimplemented here in plain shell calls (no D-Bus binding available in
## GDScript) because a PC this cartridge is plugged into for the first
## time may never have had Tatu installed at all — the fix has to be
## self-contained in the one binary guaranteed to be there.
## Idempotent (checks the conf before touching it) and best-effort: any
## failure just leaves Steam's launch to fail the way it always did before
## this existed, same as if this function didn't run.
func _ensure_ntfs_symlinks() -> void:
	var mount_point := _cartridge_root()
	var uuid_out := []
	OS.execute("findmnt", ["-no", "UUID", mount_point], uuid_out)
	var uuid := String(uuid_out[0] if uuid_out.size() > 0 else "").strip_edges()
	var source_out := []
	OS.execute("findmnt", ["-no", "SOURCE", mount_point], source_out)
	var source := String(source_out[0] if source_out.size() > 0 else "").strip_edges()
	if uuid.is_empty() or source.is_empty():
		return

	var conf_path := "/etc/udisks2/mount_options.conf"
	var marker := "[/dev/disk/by-uuid/%s]" % uuid
	var existing := FileAccess.get_file_as_string(conf_path) if FileAccess.file_exists(conf_path) else ""
	if existing.contains(marker):
		return

	_action_status.text = "Habilitando symlinks NTFS para Proton (puede pedir tu contraseña de administrador)..."
	await get_tree().process_frame

	var tmp := OS.get_cache_dir().path_join("tatu-mount-options-%s.conf" % uuid)
	var f := FileAccess.open(tmp, FileAccess.WRITE)
	f.store_string("%s\n%s\nntfs_defaults=uid=$UID,gid=$GID\n" % [existing, marker])
	f.close()

	# pkexec pops a graphical polkit prompt and blocks until it's answered —
	# fine here, this whole function already runs behind the "Registrando
	# el cartucho en Steam..." overlay the caller set up.
	OS.execute("pkexec", ["install", "-m", "0644", tmp, conf_path])
	DirAccess.remove_absolute(tmp)

	# Remounts at the same path for this label — verified live, same as
	# the manual `udisksctl unmount`/`mount` cycle this replicates.
	OS.execute("udisksctl", ["unmount", "-b", source])
	OS.execute("udisksctl", ["mount", "-b", source])

## Linux: `~/.local/share/Steam` or the older `~/.steam/steam` symlink
## target. Windows: the one non-elevated, no-registry-access location a
## default install actually lands in — matches Tatu's own Rust-side
## `steam_install_dir()` fallback for the same reason (this can't call
## into that code, see this file's header), just without the registry
## lookup GDScript has no built-in access to.
func _steam_install_dir() -> String:
	if OS.get_name() == "Windows":
		var default := "C:/Program Files (x86)/Steam"
		return default if DirAccess.dir_exists_absolute(default) else ""
	var home := OS.get_environment("HOME")
	for rel in [".local/share/Steam", ".steam/steam"]:
		var path := home.path_join(rel)
		if DirAccess.dir_exists_absolute(path):
			return path
	return ""

func _is_steam_running() -> bool:
	var output := []
	if OS.get_name() == "Windows":
		OS.execute("tasklist", ["/FI", "IMAGENAME eq steam.exe"], output)
	else:
		OS.execute("pgrep", ["-x", "steam"], output)
	return not String(output[0] if output.size() > 0 else "").strip_edges().is_empty()

## Closes Steam the same way its own tray icon "Exit" would (a normal
## process termination, not a forceful kill) and waits for it to actually
## exit — verified live (#217) that this machine's disk/process state
## doesn't always update instantly, same reasoning applies to a whole
## client shutting down its background services.
func _stop_steam() -> void:
	if not _is_steam_running():
		return
	if OS.get_name() == "Windows":
		OS.execute("taskkill", ["/IM", "steam.exe"])
	else:
		OS.execute("pkill", ["-x", "steam"])
	var attempts := 0
	while _is_steam_running() and attempts < 20:
		await get_tree().create_timer(0.5).timeout
		attempts += 1

func _start_steam(steam_dir: String) -> void:
	if OS.get_name() == "Windows":
		OS.create_process(steam_dir.path_join("steam.exe"), [])
	else:
		OS.create_process(steam_dir.path_join("steam.sh"), [])

func _tatu_local_dir() -> String:
	return OS.get_environment("HOME").path_join(".local/share/tatu")

## Matches umu-run's own resolution of UMU_LOCAL when UMU_FOLDERS_PATH is
## set (umu/umu_consts.py): `<UMU_FOLDERS_PATH>/umu`.
func _umu_local_dir() -> String:
	return _tatu_local_dir().path_join("umu")

func _umu_compat_dir() -> String:
	return _umu_local_dir().path_join("compatibilitytools")

## Copies umu-run + extracts the bundled Proton and Steam Linux Runtime from
## the cartridge (#206's Tatu-side, runtime.rs) onto this machine's local
## disk — Proton needs a real filesystem location, not everything works
## run-in-place from removable media. A no-op past the first call on a given
## machine: the marker file means every subsequent launch just reuses what's
## already deployed, cartridge or no cartridge plugged in.
func _ensure_linux_runtime_deployed() -> bool:
	var local := _tatu_local_dir()
	var deployed_marker := local.path_join(".runtime-deployed")
	if FileAccess.file_exists(deployed_marker):
		return true

	var cartridge_runtime := _cartridge_root().path_join(CARTRIDGE_RUNTIME_SUBDIR)
	var umu_run_src := cartridge_runtime.path_join("umu-run")
	if not FileAccess.file_exists(umu_run_src):
		push_warning("No Linux runtime bundled on this cartridge (#206)")
		return false

	DirAccess.make_dir_recursive_absolute(local)
	var umu_run_dst := local.path_join("umu-run")
	DirAccess.copy_absolute(umu_run_src, umu_run_dst)
	OS.execute("chmod", ["+x", umu_run_dst])

	var umu_local := _umu_local_dir()
	DirAccess.make_dir_recursive_absolute(umu_local)
	# --strip-components=1: the archive's own top-level SteamLinuxRuntime_4/
	# folder becomes $HOME/.local/share/umu's CONTENTS directly, matching
	# what umu-run itself expects (and what its own installer does).
	if not _extract_tar(cartridge_runtime.path_join(RUNTIME_ARCHIVE), umu_local, true):
		return false

	var compat_dir := _umu_compat_dir()
	DirAccess.make_dir_recursive_absolute(compat_dir)
	if not _extract_tar(cartridge_runtime.path_join(PROTON_ARCHIVE), compat_dir, false):
		return false

	var marker := FileAccess.open(deployed_marker, FileAccess.WRITE)
	marker.store_string(PROTON_DIRNAME)
	return true

func _extract_tar(archive_path: String, dest_dir: String, strip_top_level: bool) -> bool:
	var args := ["-xf", archive_path, "-C", dest_dir]
	if strip_top_level:
		args.append("--strip-components=1")
	var output := []
	var code := OS.execute("tar", args, output, true)
	if code != 0:
		push_warning("Failed to extract %s: %s" % [archive_path, output])
	return code == 0

## Registers the whole cartridge (not just the selected card) as a Steam
## library — deliberately independent of `_selected_index`, see
## `_launch_via_steam`'s own header.
func _on_add_to_steam_requested() -> void:
	await _launch_via_steam()
