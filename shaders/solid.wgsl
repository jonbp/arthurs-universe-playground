// ---- Opaque lit triangles: rockets ----
// Vertex colour alpha is "glow": 0 = lit by the stars, 1 = self-lit (engine flames).
struct SOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec4<f32>, @location(1) color: vec4<f32>) -> SOut {
    var out: SOut;
    out.clip = g.view_proj * vec4<f32>(pos.xyz, 1.0);
    out.world = pos.xyz;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: SOut) -> @location(0) vec4<f32> {
    // Flat shading: the face normal comes from screen-space derivatives, turned to face the viewer.
    let v = normalize(-in.world);
    var n = normalize(cross(dpdx(in.world), dpdy(in.world)));
    if (dot(n, v) < 0.0) {
        n = -n;
    }
    let albedo = in.color.rgb;
    // A little fill light from the camera keeps the model readable in shadow.
    var color = albedo * (g.params.w * 3.0 + 0.07 * max(dot(n, v), 0.0));
    let light_count = i32(g.params.x);
    for (var i = 0; i < light_count; i = i + 1) {
        let to_l = g.light_pos[i].xyz - in.world;
        let dist = length(to_l);
        let l = to_l / dist;
        let lc = g.light_col[i].rgb * (80.0 / max(dist, 1.0)) * g.sky_up.w;
        let diff = max(dot(n, l), 0.0);
        let h = normalize(l + v);
        let spec = pow(max(dot(n, h), 0.0), 40.0) * 0.25;
        color = color + lc * (albedo * diff + vec3<f32>(spec));
    }
    return vec4<f32>(mix(color, albedo, in.color.a), 1.0);
}
