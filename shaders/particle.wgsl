// ---- Additive camera-facing billboards: debris, dust, galaxy stars, star glows ----
struct POut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) shape: f32,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) inst: u32) -> POut {
    let p = particles[inst];
    let c = quad_corner(vi);
    let center = p.pos_size.xyz;
    var size = p.pos_size.w;
    var color = p.color.rgb;
    // Keep tiny far-away particles at >= ~1.5px, dimmed to conserve their energy,
    // but never so dim that they vanish: at the true scale of a solar system an
    // asteroid belt or a cloud of rubble is far smaller than a pixel.
    let depth = max(dot(center, g.cam_fwd.xyz), 0.001);
    let px = depth * 2.0 * g.cam_fwd.w / g.screen.y;
    let min_size = px * 1.5;
    if (size < min_size) {
        let k = size / min_size;
        color = color * max(k * k, 0.1);
        size = min_size;
    }
    let world = center + (g.cam_right.xyz * c.x + g.cam_up.xyz * c.y) * size;
    var out: POut;
    out.clip = g.view_proj * vec4<f32>(world, 1.0);
    out.uv = c;
    out.color = color;
    out.shape = p.color.w;
    return out;
}

@fragment
fn fs_main(in: POut) -> @location(0) vec4<f32> {
    let d2 = dot(in.uv, in.uv);
    if (d2 > 1.0) {
        discard;
    }
    var a: f32;
    if (in.shape > 0.5) {
        // wide glow halo
        let d = sqrt(d2);
        a = pow(1.0 - d, 3.0) * 0.6 + exp(-d * 9.0) * 0.8;
    } else {
        a = exp(-d2 * 5.0) * (1.0 - d2);
    }
    return vec4<f32>(in.color * a, 0.0);
}
