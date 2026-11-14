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
