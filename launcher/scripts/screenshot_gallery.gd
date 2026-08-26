class_name ScreenshotGallery
extends ScrollContainer
## Vertical list of the selected game's cached Steam store screenshots
## (#213), living in the launcher's right glass panel above the two action
## hints. Gamepad-first like the rest of the launcher: up/down move the
## highlight, a dedicated action enlarges it — a click still works too,
## each thumbnail is a real Button, but nothing here requires a mouse.

signal thumbnail_activated(path: String)

const THUMBNAIL_ASPECT_RATIO := 9.0 / 16.0

var _list: VBoxContainer
var _thumbnails: Array[Button] = []
var _paths: Array[String] = []
var _selected_index := 0

func _init() -> void:
	size_flags_vertical = Control.SIZE_EXPAND_FILL
	horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_list = VBoxContainer.new()
	_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	add_child(_list)

## Rebuilds the thumbnail list — called whenever the carousel's own
## selection changes, since screenshots belong to whichever game is active.
func set_screenshots(paths: Array[String]) -> void:
	for thumb in _thumbnails:
		thumb.queue_free()
	_thumbnails.clear()
	_paths = paths
	_selected_index = 0
	for path in paths:
		var thumb := Button.new()
		thumb.flat = true
		thumb.focus_mode = Control.FOCUS_NONE
		thumb.size_flags_horizontal = Control.SIZE_EXPAND_FILL

		var texture_rect := TextureRect.new()
		texture_rect.set_anchors_preset(Control.PRESET_FULL_RECT)
		texture_rect.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
		texture_rect.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_COVERED
		texture_rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
		var image := Image.new()
		if image.load(path) == OK:
			texture_rect.texture = ImageTexture.create_from_image(image)
		else:
			push_warning("Cannot load screenshot at %s" % path)
		thumb.add_child(texture_rect)

		var index := _thumbnails.size()
		thumb.pressed.connect(func() -> void:
			_selected_index = index
			_update_highlight()
			thumbnail_activated.emit(path))
		_list.add_child(thumb)
		_thumbnails.append(thumb)
	_update_highlight()

## Sizes each thumbnail off the panel's own width, same ratio-of-window
## rule as everything else in the launcher — called by main.gd's own
## _resize_layout() with the right glass panel's current content width.
func resize(panel_width: float) -> void:
	var height := panel_width * THUMBNAIL_ASPECT_RATIO
	for thumb in _thumbnails:
		thumb.custom_minimum_size = Vector2(0, height)

func move_selection(delta: int) -> void:
	if _thumbnails.is_empty():
		return
	_selected_index = clampi(_selected_index + delta, 0, _thumbnails.size() - 1)
	_update_highlight()
	ensure_control_visible(_thumbnails[_selected_index])

func has_screenshots() -> bool:
	return not _thumbnails.is_empty()

func selected_path() -> String:
	if _paths.is_empty():
		return ""
	return _paths[_selected_index]

func _update_highlight() -> void:
	for i in _thumbnails.size():
		_thumbnails[i].modulate = Color.WHITE if i == _selected_index else Color(1, 1, 1, 0.55)
