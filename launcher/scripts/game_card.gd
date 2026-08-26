class_name GameCard
extends Button
## One cover-art card in the launcher grid (#204). Hover AND keyboard/gamepad
## focus trigger the same scale animation, so a gamepad-only player gets the
## same feedback a mouse player does.

signal launch_requested(app_id: int)

const HOVER_SCALE := Vector2(1.08, 1.08)
const HOVER_DURATION := 0.12

var _app_id: int = 0
var _tween: Tween
var _art: TextureRect
var _name_label: Label

func _init() -> void:
	custom_minimum_size = Vector2(200, 260)
	flat = true
	focus_mode = Control.FOCUS_ALL

	var box := VBoxContainer.new()
	box.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(box)

	_art = TextureRect.new()
	_art.custom_minimum_size = Vector2(0, 200)
	_art.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	_art.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	_art.mouse_filter = Control.MOUSE_FILTER_IGNORE
	box.add_child(_art)

	_name_label = Label.new()
	_name_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_name_label.autowrap_mode = TextServer.AUTOWRAP_WORD
	_name_label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	box.add_child(_name_label)

func _ready() -> void:
	pivot_offset = size / 2.0
	resized.connect(func() -> void: pivot_offset = size / 2.0)

	pressed.connect(func() -> void: launch_requested.emit(_app_id))
	mouse_entered.connect(_grow)
	focus_entered.connect(_grow)
	mouse_exited.connect(_maybe_shrink)
	focus_exited.connect(_maybe_shrink)

func setup(app_id: int, display_name: String, art_path: String) -> void:
	_app_id = app_id
	_name_label.text = display_name
	_load_art(art_path)

func _load_art(path: String) -> void:
	if path.is_empty():
		return
	var image := Image.new()
	var err := image.load(path)
	if err != OK:
		push_warning("Cannot load cover art at %s: error %d" % [path, err])
		return
	_art.texture = ImageTexture.create_from_image(image)

func _grow() -> void:
	_animate_scale(HOVER_SCALE)

func _maybe_shrink() -> void:
	if not is_hovered() and not has_focus():
		_animate_scale(Vector2.ONE)

func _animate_scale(target: Vector2) -> void:
	if _tween:
		_tween.kill()
	_tween = create_tween()
	_tween.tween_property(self, "scale", target, HOVER_DURATION).set_trans(Tween.TRANS_SINE)
