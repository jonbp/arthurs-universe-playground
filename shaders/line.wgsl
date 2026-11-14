// ---- Orbit trails, trajectory predictions and aim arrows (additive line list) ----
struct LOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec4<f32>, @location(1) color: vec4<f32>) -> LOut {
    var out: LOut;
    out.clip = g.view_proj * vec4<f32>(pos.xyz, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: LOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color.rgb * in.color.a, 0.0);
}
