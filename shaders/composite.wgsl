// ---- Final composite: gravitational lensing + bloom + tonemapping ----
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var hdr_tex: texture_2d<f32>;
@group(0) @binding(2) var hdr_samp: sampler;

struct COut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> COut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var out: COut;
    out.clip = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

/// 1 inside a UI panel that blurs what's behind it, 0 outside (rounded corners, 1 px soft edge).
fn ui_blur_mask(px: vec2<f32>) -> f32 {
    var m = 0.0;
    let n = i32(g.ui_blur_params.x);
    let r = g.ui_blur_params.y;
    for (var i = 0; i < n; i = i + 1) {
        let rect = g.ui_blur[i] * vec4<f32>(g.screen.xy, g.screen.xy);
        let c = (rect.xy + rect.zw) * 0.5;
        let half = (rect.zw - rect.xy) * 0.5;
        // Signed distance to a rounded rectangle.
        let q = abs(px - c) - half + vec2<f32>(r);
        let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
        m = max(m, clamp(0.5 - d, 0.0, 1.0));
    }
    return m;
}

/// A soft blur of the scene: a 3x3 tent over a small mip, smoothed further by a smaller one.
fn blurred_scene(uv: vec2<f32>) -> vec3<f32> {
    let texel = 8.0 / g.screen.xy;
    var acc = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let w = (2.0 - abs(f32(x))) * (2.0 - abs(f32(y)));
            let o = vec2<f32>(f32(x), f32(y)) * texel * 1.5;
            acc = acc + textureSampleLevel(hdr_tex, hdr_samp, uv + o, 3.0).rgb * w;
            wsum = wsum + w;
        }
    }
    return mix(acc / wsum, textureSampleLevel(hdr_tex, hdr_samp, uv, 4.0).rgb, 0.35);
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(in: COut) -> @location(0) vec4<f32> {
    let aspect = g.screen.x / g.screen.y;
    var uv = in.uv;
    var ring_glow = vec3<f32>(0.0);
    var shadow = 1.0;

    // Schwarzschild-style point lens: beta = theta - theta_E^2 / theta
    let bh_count = i32(g.params.z);
    for (var i = 0; i < bh_count; i = i + 1) {
        let bh = g.black_holes[i];
        let rel = (in.uv - bh.xy) * vec2<f32>(aspect, 1.0);
        let r = max(length(rel), 1e-5);
        let rs = bh.z;
        let theta_e = rs * 2.2;
        let fade = 1.0 - smoothstep(rs * 10.0, rs * 25.0, r);
        let deflect = theta_e * theta_e / r * fade;
        let dir = rel / r;
        uv = uv - dir * deflect / vec2<f32>(aspect, 1.0);
        // photon ring
        ring_glow = ring_glow + vec3<f32>(1.0, 0.8, 0.6) * exp(-pow((r - theta_e * 0.82) / (rs * 0.06), 2.0)) * 0.35 * bh.w;
        shadow = shadow * smoothstep(rs * 0.98, rs * 1.15, r);
    }

    uv = clamp(uv, vec2<f32>(0.0005), vec2<f32>(0.9995));
    var col = textureSampleLevel(hdr_tex, hdr_samp, uv, 0.0).rgb;
    // Frosted glass behind side panels: the scene there is blurred (the panel's tint is drawn by the UI).
    let blur = ui_blur_mask(in.uv * g.screen.xy);
    if (blur > 0.0) {
        col = mix(col, blurred_scene(uv), blur);
    }

    // Cheap wide bloom from the blit-generated mip chain
    var bloom = vec3<f32>(0.0);
    bloom = bloom + textureSampleLevel(hdr_tex, hdr_samp, uv, 1.0).rgb * 0.08;
    bloom = bloom + textureSampleLevel(hdr_tex, hdr_samp, uv, 2.0).rgb * 0.12;
    bloom = bloom + textureSampleLevel(hdr_tex, hdr_samp, uv, 3.0).rgb * 0.16;
    bloom = bloom + textureSampleLevel(hdr_tex, hdr_samp, uv, 4.0).rgb * 0.2;
    bloom = bloom + textureSampleLevel(hdr_tex, hdr_samp, uv, 5.0).rgb * 0.22;
    bloom = bloom + textureSampleLevel(hdr_tex, hdr_samp, uv, 6.0).rgb * 0.22;
    // only the bright part blooms
    bloom = max(bloom - vec3<f32>(0.08), vec3<f32>(0.0));
    col = col + bloom * g.params.y;
    col = col * shadow + ring_glow;

    col = aces(col * g.screen.w);
    // vignette
    let v = in.uv - 0.5;
    col = col * (1.0 - dot(v, v) * 0.35);
    // gamma (swapchain is UNORM)
    col = pow(col, vec3<f32>(1.0 / 2.2));
    return vec4<f32>(col, 1.0);
}
