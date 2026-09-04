// Single-pass blur for the background art behind the launcher's side
// panels — a "frosted glass" backdrop, not a live screen-space blur (this
// samples the TextureRect's own texture, not whatever else might render
// behind it, which is simpler and avoids draw-order issues between UI
// siblings). Gaussian-weighted 7x7 kernel: a flat box average at this tap
// spacing produced visible moiré on repeating art (checkerboards, grid
// patterns) — weighting samples by distance from center fixes that.
shader_type canvas_item;

uniform float blur_amount : hint_range(0.0, 10.0) = 2.5;

void fragment() {
	vec2 pixel_size = TEXTURE_PIXEL_SIZE * blur_amount;
	vec4 sum = vec4(0.0);
	float total_weight = 0.0;
	for (int x = -3; x <= 3; x++) {
		for (int y = -3; y <= 3; y++) {
			float d2 = float(x * x + y * y);
			float weight = exp(-d2 / 8.0);
			sum += texture(TEXTURE, UV + vec2(float(x), float(y)) * pixel_size) * weight;
			total_weight += weight;
		}
	}
	COLOR = sum / total_weight;
}
