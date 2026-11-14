// ---- Planetary rings & black hole accretion disks ----
struct RingOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) plane: vec2<f32>,
    @location(2) @interpolate(flat) inst: u32,
};

fn ring_basis(n: vec3<f32>) -> mat3x3<f32> {
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.95);
    let t = normalize(cross(up, n));
    let bt = cross(n, t);
    return mat3x3<f32>(t, bt, n);
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) inst: u32) -> RingOut {
    let r = rings[inst];
    let c = quad_corner(vi) * r.params.x;
    let basis = ring_basis(normalize(r.normal.xyz));
    let world = r.center.xyz + basis[0] * c.x + basis[1] * c.y;
    var out: RingOut;
    out.clip = g.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    out.plane = c;
    out.inst = inst;
    return out;
}

@fragment
fn fs_main(in: RingOut) -> @location(0) vec4<f32> {
    let r = rings[in.inst];
    let dist = length(in.plane);
    let inner = r.normal.w;
    let outer = r.params.x;
    if (dist < inner || dist > outer) {
        discard;
    }
    let t = (dist - inner) / (outer - inner);
    let n = normalize(r.normal.xyz);
    let seed = r.params.z;
    let time = g.screen.z;

    if (r.params.y > 0.5) {
        // ---- accretion disk ----
        let ang = atan2(in.plane.y, in.plane.x);
        // Keplerian shear: inner parts rotate faster
        let omega = 2.5 / pow(max(dist / inner, 1.0), 1.5);
        let swirl = ang + time * omega + log(dist) * 3.0;
        let streaks = fbm(vec3<f32>(cos(swirl) * 3.0, sin(swirl) * 3.0, dist * 0.6 / inner + seed), 4);
        let heat = pow(1.0 - t, 1.6);
        var col = heat_color(0.25 + 0.75 * heat) * (0.15 + 2.2 * streaks * streaks);
        col = mix(col, vec3<f32>(0.8, 0.9, 1.0), smoothstep(0.85, 1.0, heat) * 0.6);
        // Doppler beaming: material moving toward the camera is brighter
        let basis = ring_basis(n);
        let radial = normalize(basis[0] * in.plane.x + basis[1] * in.plane.y);
        let vel = cross(n, radial);
        let toward = dot(vel, normalize(-in.world));
        let beam = pow(1.0 + 0.6 * toward, 3.0);
        let edge = smoothstep(0.0, 0.08, t) * smoothstep(1.0, 0.6, t);
        let intensity = r.params.w * edge * beam * (0.25 + 1.6 * heat) * 0.45;
        return vec4<f32>(col * intensity, 0.0);
    }

    // ---- planetary ring ----
    let bands = fbm(vec3<f32>(dist * 6.0 / outer + seed, seed, 0.0), 5);
    let gaps = smoothstep(0.32, 0.4, bands) * (1.0 - 0.8 * smoothstep(0.58, 0.6, t) * smoothstep(0.64, 0.62, t));
    let alpha = r.color.a * gaps * smoothstep(0.0, 0.05, t) * smoothstep(1.0, 0.9, t);
    var col = r.color.rgb * (0.7 + 0.6 * bands);

    // lighting + planet shadow from the brightest light
    var lit = vec3<f32>(g.params.w);
    let light_count = i32(g.params.x);
    for (var i = 0; i < light_count; i = i + 1) {
        let to_l = g.light_pos[i].xyz - in.world;
        let d = length(to_l);
        let l = to_l / d;
        // ray-sphere test against the planet
        let oc = in.world - r.center.xyz;
        let bq = dot(oc, l);
        let cq = dot(oc, oc) - r.center.w * r.center.w;
        let disc = bq * bq - cq;
        let shadow = select(1.0, 0.08, disc > 0.0 && bq < 0.0);
        lit = lit + g.light_col[i].rgb * (80.0 / max(d, 1.0)) * (0.35 + 0.65 * abs(dot(n, l))) * shadow;
    }
    col = col * lit;
    return vec4<f32>(col * alpha, alpha);
}
