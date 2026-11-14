// ---- Procedural starfield + faint nebula background ----
struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> SkyOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    let ndc = uv * 2.0 - 1.0;
    var out: SkyOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.ndc = ndc;
    return out;
}

fn star_layer(dir: vec3<f32>, scale: f32, density: f32, px: f32) -> vec3<f32> {
    let q = dir * scale;
    let cell = floor(q);
    let f = fract(q);
    let h = hash33(cell);
    if (h.x > density) {
        return vec3<f32>(0.0);
    }
    let pos = vec3<f32>(0.25) + h * 0.5;
    let d = length(f - pos);
    // Energy-conserving anti-aliasing: a star never gets narrower than ~1px;
    // when it would be smaller it is spread over a pixel and dimmed instead,
    // so it fades smoothly rather than twinkling/aliasing as the camera moves.
    let w0 = 0.03;
    let w = max(w0, px * scale * 0.9);
    let energy = (w0 / w) * (w0 / w);
    let core = exp(-(d * d) / (w * w));
    let temp = hash3(cell + vec3<f32>(7.0));
    let tint = mix(vec3<f32>(1.0, 0.75, 0.55), vec3<f32>(0.75, 0.85, 1.0), temp);
    let bright = pow(hash3(cell + vec3<f32>(3.0)), 8.0) * 4.0 + 0.03;
    return tint * core * bright * energy;
}

@fragment
fn fs_main(in: SkyOut) -> @location(0) vec4<f32> {
    // Reverse-Z: far plane is z = 0; point at depth ~1 is near.
    let world_h = g.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let dir = normalize(world_h.xyz / world_h.w);
    let px = 2.0 * g.cam_fwd.w / g.screen.y;
    let star_amount = g.cam_right.w;

    var col = vec3<f32>(0.0);
    // Two sparse layers: a few bright stars and a faint scattering.
    col = col + star_layer(dir, 45.0, 0.22, px) * 1.4;
    col = col + star_layer(dir, 110.0, 0.07, px) * 0.6;
    col = col * star_amount;

    // Smooth, low-frequency milky-way haze and nebula clouds (no fine detail).
    let band_axis = normalize(vec3<f32>(0.3, 1.0, 0.2));
    let band = exp(-pow(dot(dir, band_axis) * 3.0, 2.0));
    let neb = fbm(dir * 2.5 + vec3<f32>(11.0), 4);
    let neb2 = fbm(dir * 4.0 + vec3<f32>(3.0), 3);
    var haze = vec3<f32>(0.07, 0.06, 0.09) * band * (0.4 + smoothstep(0.35, 0.8, neb));
    haze = haze + vec3<f32>(0.03, 0.02, 0.06) * smoothstep(0.55, 0.85, neb2) * 0.6;
    haze = haze + vec3<f32>(0.05, 0.025, 0.02) * smoothstep(0.62, 0.9, fbm(dir * 3.0 + vec3<f32>(30.0), 3)) * 0.4;
    col = col + haze * (0.35 + 0.65 * min(star_amount, 1.5));

    // Inside an atmosphere the daytime sky hides the stars: brighter and paler
    // towards the horizon, deeper overhead.
    if (g.sky.a > 0.001) {
        let h = dot(dir, g.sky_up.xyz);
        let horizon = pow(1.0 - clamp(h, 0.0, 1.0), 4.0);
        let lum = dot(g.sky.rgb, vec3<f32>(0.3, 0.5, 0.2));
        let air = g.sky.rgb * (0.75 + 0.6 * horizon) + vec3<f32>(lum * 0.45 * horizon);
        col = mix(col, air, g.sky.a);
    }
    return vec4<f32>(col, 1.0);
}
