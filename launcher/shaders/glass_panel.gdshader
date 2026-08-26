// Frosted-glass side panel: blurs whatever is actually behind it on
// screen (the carousel, cards sliding past) rather than a static image, so
// a card visibly softens as it passes under the panel instead of just
// disappearing at a hard edge.
shader_type canvas_item;

uniform sampler2D screen_texture : hint_screen_texture, repeat_disable, filter_linear;
uniform float blur_amount : hint_range(0.0, 20.0) = 6.0;
uniform vec4 tint : source_color = vec4(0.08, 0.08, 0.1, 0.6);

void fragment() {
	vec2 pixel_size = SCREEN_PIXEL_SIZE * blur_amount;
	vec4 sum = vec4(0.0);
	float total_weight = 0.0;
	for (int x = -3; x <= 3; x++) {
		for (int y = -3; y <= 3; y++) {
			float d2 = float(x * x + y * y);
			float weight = exp(-d2 / 8.0);
			sum += texture(screen_texture, SCREEN_UV + vec2(float(x), float(y)) * pixel_size) * weight;
			total_weight += weight;
		}
	}
	vec3 blurred = (sum / total_weight).rgb;
	COLOR = vec4(mix(blurred, tint.rgb, tint.a), 1.0);
}
