class_name SteamCefClient
extends RefCounted
## GDScript port of CapyDeploy's crates/steam/src/cef.rs and its Decky
## frontend (decky-capydeploy/src/eventPoller.tsx) — talks Chrome DevTools
## Protocol to Steam's own embedded Chromium (127.0.0.1:8080) and evaluates
## `SteamClient.Apps.*` JS calls against the ALREADY-RUNNING Steam client.
## No `shortcuts.vdf` writer: Steam applies these live, the same effect a
## player clicking through its own UI would have.
##
## Call shapes and the clear-before-set artwork sequencing are copied 1:1
## from the Decky frontend — a real, shipped caller of these same
## SteamClient methods running inside Steam's own JS context. This
## launcher has no such context (it's a standalone binary, not a Steam
## plugin), so it drives the identical calls remotely over CDP instead.
## Don't re-derive the protocol or the call order; both were verified live
## by that other project already.

const DEBUG_HOST := "127.0.0.1"
const DEBUG_PORT := 8080
const CONNECT_TIMEOUT_MS := 3000
const EVAL_TIMEOUT_MS := 10000

# Matches SteamClient.Apps's own asset-type enum — cross-checked against
# both cef.rs's CEF_ASSET_* constants and eventPoller.tsx's ASSET_TYPE.
const ASSET_GRID_PORTRAIT := 0
const ASSET_HERO := 1
const ASSET_LOGO := 2
const ASSET_GRID_LANDSCAPE := 3
const ASSET_ICON := 4

var _next_id := 1

## Cheap probe: true if something answers on the debug port at all. Doesn't
## confirm a SharedJSContext/SP tab actually exists — callers still have to
## handle a later _evaluate() failure regardless.
static func is_available() -> bool:
	var stream := StreamPeerTCP.new()
	stream.connect_to_host(DEBUG_HOST, DEBUG_PORT)
	var deadline := Time.get_ticks_msec() + CONNECT_TIMEOUT_MS
	while stream.get_status() == StreamPeerTCP.STATUS_CONNECTING and Time.get_ticks_msec() < deadline:
		stream.poll()
	var ok := stream.get_status() == StreamPeerTCP.STATUS_CONNECTED
	stream.disconnect_from_host()
	return ok

## Registers `exe` as a Non-Steam shortcut and names it — mirrors
## handleCreateShortcut's own call order (AddShortcut then
## SetShortcutName). Compat tool is a separate call, left to the caller:
## unlike the Decky reference, this launcher already knows up front
## whether the target needs Proton, no ".exe" sniffing required. Returns
## the new Steam appid, or 0 on any failure.
func add_shortcut(name: String, exe: String, start_dir: String) -> int:
	var js := "SteamClient.Apps.AddShortcut(%s, %s, %s, %s)" % [
		JSON.stringify(name), JSON.stringify(exe), JSON.stringify(start_dir), JSON.stringify("")
	]
	var eval := await _evaluate(js)
	if not eval["ok"] or eval["value"] == null:
		return 0
	var app_id := int(eval["value"])
	if app_id == 0:
		return 0
	await _evaluate("SteamClient.Apps.SetShortcutName(%d, %s)" % [app_id, JSON.stringify(name)])
	return app_id

func specify_compat_tool(app_id: int, tool_name: String) -> bool:
	var eval := await _evaluate(
		"SteamClient.Apps.SpecifyCompatTool(%d, %s)" % [app_id, JSON.stringify(tool_name)]
	)
	return eval["ok"]

## Clears then re-applies artwork for one asset slot — the 500ms gap
## matches the Decky frontend's own sequencing (a bare Set right after
## Clear left stale art on screen there); ported as-is rather than
## re-testing whether this launcher's own timing needs it.
func set_custom_artwork(app_id: int, base64_png: String, asset_type: int) -> bool:
	await _evaluate("SteamClient.Apps.ClearCustomArtworkForApp(%d, %d)" % [app_id, asset_type])
	await _sleep(0.5)
	var eval := await _evaluate(
		"SteamClient.Apps.SetCustomArtworkForApp(%d, %s, \"png\", %d)"
		% [app_id, JSON.stringify(base64_png), asset_type]
	)
	return eval["ok"]

func remove_shortcut(app_id: int) -> bool:
	var eval := await _evaluate("SteamClient.Apps.RemoveShortcut(%d)" % app_id)
	return eval["ok"]

func _sleep(seconds: float) -> void:
	await (Engine.get_main_loop() as SceneTree).create_timer(seconds).timeout

## GET /json off the debug port, returns the raw tab array (empty on any
## failure). No Node/HTTPRequest here — this class has no scene-tree
## parent to host one — HTTPClient is Godot's own non-Node primitive for
## exactly this case.
func _get_tabs() -> Array:
	var client := HTTPClient.new()
	if client.connect_to_host(DEBUG_HOST, DEBUG_PORT) != OK:
		return []
	var loop := Engine.get_main_loop() as SceneTree
	var deadline := Time.get_ticks_msec() + CONNECT_TIMEOUT_MS
	while client.get_status() in [HTTPClient.STATUS_CONNECTING, HTTPClient.STATUS_RESOLVING]:
		client.poll()
		if Time.get_ticks_msec() > deadline:
			return []
		await loop.process_frame

	if client.get_status() != HTTPClient.STATUS_CONNECTED:
		return []
	var headers := PackedStringArray(["Host: %s:%d" % [DEBUG_HOST, DEBUG_PORT]])
	if client.request(HTTPClient.METHOD_GET, "/json", headers) != OK:
		return []
	while client.get_status() == HTTPClient.STATUS_REQUESTING:
		client.poll()
		await loop.process_frame

	if not client.get_status() in [HTTPClient.STATUS_BODY, HTTPClient.STATUS_CONNECTED]:
		return []

	var body := PackedByteArray()
	while client.get_status() == HTTPClient.STATUS_BODY:
		client.poll()
		var chunk := client.read_response_body_chunk()
		if chunk.is_empty():
			await loop.process_frame
		else:
			body.append_array(chunk)

	var parsed: Variant = JSON.parse_string(body.get_string_from_utf8())
	return parsed if typeof(parsed) == TYPE_ARRAY else []

## Prefers a `SharedJSContext` tab, falls back to `SP` — mirrors
## `CefClient::find_js_context` exactly.
func _find_js_context(tabs: Array) -> Dictionary:
	var fallback := {}
	for tab in tabs:
		var title := String((tab as Dictionary).get("title", ""))
		if title == "SharedJSContext":
			return tab
		if title == "SP" and fallback.is_empty():
			fallback = tab
	return fallback

## Opens a WebSocket to the JS-context tab, sends Runtime.evaluate for
## `js_expr`, and waits for the matching response. `ok` is false on any
## transport failure, timeout, or JS exception — a legitimate void call
## still reports `ok = true` with `value = null`.
func _evaluate(js_expr: String) -> Dictionary:
	var tabs := await _get_tabs()
	if tabs.is_empty():
		push_warning("Steam CEF: no debug tabs at %s:%d" % [DEBUG_HOST, DEBUG_PORT])
		return {"ok": false, "value": null}
	var tab := _find_js_context(tabs)
	if tab.is_empty():
		push_warning("Steam CEF: no SharedJSContext/SP tab found")
		return {"ok": false, "value": null}
	var ws_url := String(tab.get("webSocketDebuggerUrl", ""))
	if ws_url.is_empty():
		return {"ok": false, "value": null}

	var loop := Engine.get_main_loop() as SceneTree
	var socket := WebSocketPeer.new()
	if socket.connect_to_url(ws_url) != OK:
		return {"ok": false, "value": null}

	var deadline := Time.get_ticks_msec() + EVAL_TIMEOUT_MS
	while socket.get_ready_state() == WebSocketPeer.STATE_CONNECTING and Time.get_ticks_msec() < deadline:
		socket.poll()
		await loop.process_frame
	if socket.get_ready_state() != WebSocketPeer.STATE_OPEN:
		return {"ok": false, "value": null}

	var request_id := _next_id
	_next_id += 1
	socket.send_text(JSON.stringify({
		"id": request_id,
		"method": "Runtime.evaluate",
		"params": {"expression": js_expr, "awaitPromise": true},
	}))

	while Time.get_ticks_msec() < deadline:
		socket.poll()
		while socket.get_available_packet_count() > 0:
			var parsed: Variant = JSON.parse_string(socket.get_packet().get_string_from_utf8())
			if typeof(parsed) != TYPE_DICTIONARY or int((parsed as Dictionary).get("id", -1)) != request_id:
				continue
			socket.close()
			var result: Dictionary = (parsed as Dictionary).get("result", {})
			if result.has("exceptionDetails"):
				push_warning(
					"Steam CEF eval failed for `%s`: %s"
					% [js_expr, JSON.stringify(result["exceptionDetails"])]
				)
				return {"ok": false, "value": null}
			return {"ok": true, "value": (result.get("result", {}) as Dictionary).get("value")}
		if socket.get_ready_state() != WebSocketPeer.STATE_OPEN:
			break
		await loop.process_frame

	socket.close()
	push_warning("Steam CEF: evaluate timed out for `%s`" % js_expr)
	return {"ok": false, "value": null}
