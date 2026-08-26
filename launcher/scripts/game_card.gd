class_name GameCard
extends Button
## One cover-art card in the launcher's carousel (#204). Purely visual plus
## click-to-select — the carousel (main.gd) drives which card is "selected"
## (scale/dim) and owns the only two actions that exist (launch, add to
## Steam), since those apply to whichever card sits centered, not to
## whichever one has mouse/keyboard focus.

signal clicked(index: int)

const SELECTED_SCALE := Vector2(1.25, 1.25)
const IDLE_SCALE := Vector2(0.85, 0.85)
const SELECT_DURATION := 0.15

var index: int = 0
var _art: TextureRect
var _name_label: Label
var _tween: Tween

func _init() -> void:
	custom_minimum_size = Vector2(200, 260)
	size = custom_minimum_size
	pivot_offset = custom_minimum_size / 2.0
	flat = true
	focus_mode = Control.FOCUS_NONE

	# Button is a plain Control, not a Container — it never stretches a
	# child to fill it. Anchor explicitly or content shrinks to its own
	# minimum size and renders bunched at the top-left corner.
	var box := VBoxContainer.new()
	box.mouse_filter = Control.MOUSE_FILTER_IGNORE
	box.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(box)

	_art = TextureRect.new()
	_art.custom_minimum_size = Vector2(0, 200)
	_art.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_art.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	_art.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	_art.mouse_filter = Control.MOUSE_FILTER_IGNORE
	box.add_child(_art)

	_name_label = Label.new()
	_name_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_name_label.autowrap_mode = TextServer.AUTOWRAP_WORD
	_name_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_name_label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	box.add_child(_name_label)

func _ready() -> void:
	pressed.connect(func() -> void: clicked.emit(index))

func setup(i: int, display_name: String, art_path: String) -> void:
	index = i
	_name_label.text = display_name
	_load_art(art_path)

## Called by the carousel every time the selection changes — this card does
## not track its own selected state.
func set_selected(selected: bool) -> void:
	var target_scale := SELECTED_SCALE if selected else IDLE_SCALE
	var target_modulate := Color.WHITE if selected else Color(1, 1, 1, 0.5)
	if _tween:
		_tween.kill()
	_tween = create_tween().set_parallel(true)
	_tween.tween_property(self, "scale", target_scale, SELECT_DURATION).set_trans(Tween.TRANS_SINE)
	_tween.tween_property(self, "modulate", target_modulate, SELECT_DURATION)

func _load_art(path: String) -> void:
	if path.is_empty():
		return
	var image := Image.new()
	var err := image.load(path)
	if err != OK:
		push_warning("Cannot load cover art at %s: error %d" % [path, err])
		return
	_art.texture = ImageTexture.create_from_image(image)
