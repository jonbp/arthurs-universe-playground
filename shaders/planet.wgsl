// ---- Planet / star / black hole sphere shader ----
// Body kinds (must match BodyKind in src/body.rs)
const K_STAR: i32 = 0;
const K_ROCKY: i32 = 1;
const K_EARTH: i32 = 2;
const K_DESERT: i32 = 3;
const K_CLOUDY: i32 = 4;
const K_GAS: i32 = 5;
const K_ICEGIANT: i32 = 6;
const K_LAVA: i32 = 7;
const K_ICE: i32 = 8;
const K_ASTEROID: i32 = 9;
const K_NEUTRON: i32 = 10;
const K_BLACKHOLE: i32 = 11;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) local: vec3<f32>,
    @location(2) @interpolate(flat) inst: u32,
};

fn body_kind(b: GpuBody) -> i32 {
    return i32(b.col_a.w + 0.5);
}

fn paint_sample(p: vec3<f32>, layer: f32) -> vec4<f32> {
    let uv = sphere_uv(p);
    let s = textureSampleLevel(paint_tex, paint_samp, uv, i32(layer + 0.5), 0.0);
    return select(s, vec4<f32>(0.5, 0.0, 0.0, 0.0), layer < 0.5);
}

// Procedural base elevation in roughly [-1, 1]
fn base_height(kind: i32, p: vec3<f32>, seed: f32) -> f32 {
    let sp = p + vec3<f32>(seed * 7.13, seed * 3.71, seed * 5.17);
    switch kind {
        case 1: { // rocky, cratered
            return (fbm(sp * 2.5, 4) - 0.5) * 0.6 + craters(p, 3.0, seed) + 0.5 * craters(p, 8.0, seed + 3.0);
        }
        case 2: { // earthlike continents + mountains
            let c = fbm(sp * 1.4, 5) * 2.0 - 1.0;
            let m = ridged(sp * 4.0, 4);
            return c + max(c, 0.0) * m * 0.8;
        }
        case 3: { // desert
            return (fbm(sp * 2.0, 5) - 0.5) * 1.2 + 0.6 * craters(p, 5.0, seed);
        }
        case 7: { // lava
            return (fbm(sp * 2.5, 4) - 0.5);
        }
        case 8: { // ice
            return (fbm(sp * 3.0, 3) - 0.5) * 0.4 - ridged(sp * 5.0, 3) * 0.3;
        }
        case 9: { // asteroid - lumpy potato
            return (fbm(sp * 1.3, 3) - 0.5) * 3.0 + craters(p, 3.5, seed);
        }
        default: {
            return 0.0;
        }
    }
}

// Elevation used both for displacement and land/sea decisions
fn elevation(kind: i32, p: vec3<f32>, b: GpuBody, pt: vec4<f32>) -> f32 {
    return base_height(kind, p, b.col_b.w) + (pt.r - 0.5) * 2.0;
}

// Relative radial displacement
fn displacement(kind: i32, p: vec3<f32>, b: GpuBody, pt: vec4<f32>) -> f32 {
    if (kind == K_STAR || kind == K_NEUTRON || kind == K_BLACKHOLE) {
        return 0.0;
    }
    let paint_h = (pt.r - 0.5) * 2.0;
    var e = base_height(kind, p, b.col_b.w);
    if (kind == K_EARTH) {
        e = max(e + paint_h, b.params.y);
        return e * b.params.x;
    }
    return e * b.params.x + paint_h * 0.07;
}

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @builtin(instance_index) inst: u32) -> VsOut {
    let b = bodies[inst];
    let kind = body_kind(b);
    let p = normalize(pos);
    let pt = paint_sample(p, b.col_c.w);
    let r = b.pos_radius.w * (1.0 + displacement(kind, p, b, pt));
    let world = b.pos_radius.xyz + quat_rotate(b.rot, p * r);
    var out: VsOut;
    out.clip = g.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    out.local = p;
    out.inst = inst;
    return out;
}

// Mikkelsen-style bump mapping from screen-space derivatives
fn perturb_normal(pos: vec3<f32>, n: vec3<f32>, h: f32) -> vec3<f32> {
    let dpx = dpdx(pos);
    let dpy = dpdy(pos);
    let dhx = dpdx(h);
    let dhy = dpdy(h);
    let r1 = cross(dpy, n);
    let r2 = cross(n, dpx);
    let det = dot(dpx, r1);
    let grad = sign(det) * (dhx * r1 + dhy * r2);
    let v = abs(det) * n - grad;
    let len = length(v);
    return select(n, v / len, len > 1e-12);
}

struct Surf {
    albedo: vec3<f32>,
    emissive: vec3<f32>,
    spec: f32,
};

fn material_color(id: i32) -> vec3<f32> {
    switch id {
        case 1: { return vec3<f32>(0.02, 0.09, 0.28); }   // water
        case 2: { return vec3<f32>(0.08, 0.03, 0.02); }   // lava crust
        case 3: { return vec3<f32>(0.85, 0.9, 0.97); }    // ice
        case 4: { return vec3<f32>(0.08, 0.3, 0.07); }    // vegetation
        case 5: { return vec3<f32>(0.32, 0.3, 0.28); }    // rock
        case 6: { return vec3<f32>(0.76, 0.62, 0.38); }   // sand
        case 7: { return vec3<f32>(0.03, 0.025, 0.02); }  // scorched
        default: { return vec3<f32>(0.5); }
    }
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let b = bodies[in.inst];
    let kind = body_kind(b);
    let p = normalize(in.local);
    let seed = b.col_b.w;
    let radius = b.pos_radius.w;
    let time = g.screen.z;
    let pt = paint_sample(p, b.col_c.w);
    let elev = elevation(kind, p, b, pt);
    let disp = displacement(kind, p, b, pt);

    // Bump normal in object space, then to world.
    let bump_scale = select(1.0, 0.0, kind == K_EARTH && elev < b.params.y);
    let n_obj = perturb_normal(p * radius, p, disp * radius * 2.5 * bump_scale);
    let n = normalize(quat_rotate(b.rot, n_obj));
    let geo = normalize(quat_rotate(b.rot, p));
    let v = normalize(-in.world);
    let ndv = max(dot(geo, v), 0.0);

    let sp = p + vec3<f32>(seed * 7.13, seed * 3.71, seed * 5.17);
    var s: Surf;
    s.albedo = b.col_a.rgb;
    s.emissive = vec3<f32>(0.0);
    s.spec = 0.0;
    var clouds = 0.0;

    switch kind {
        case 0: { // star
            let gran = fbm(sp * 14.0 + vec3<f32>(time * 0.05), 4);
            let spots = smoothstep(0.62, 0.72, fbm(sp * 3.0 + vec3<f32>(time * 0.01), 3));
            let limb = 0.35 + 0.65 * pow(ndv, 0.6);
            let c = mix(b.col_a.rgb, b.col_b.rgb, spots * 0.7);
            s.albedo = vec3<f32>(0.0);
            s.emissive = c * b.extra.x * limb * (0.75 + 0.5 * gran);
        }
        case 1: { // rocky
            let maria = smoothstep(0.45, 0.6, fbm(sp * 1.5, 4));
            let grain = fbm(sp * 20.0, 3);
            s.albedo = mix(b.col_a.rgb, b.col_b.rgb, maria) * (0.8 + 0.4 * grain);
        }
        case 2: { // earthlike
            let sea = b.params.y;
            if (elev < sea) {
                let depth = clamp((sea - elev) * 2.0, 0.0, 1.0);
                s.albedo = mix(b.col_c.rgb * 1.6, b.col_c.rgb * 0.6, depth);
                s.spec = 1.0;
            } else {
                let hgt = elev - sea;
                let dry = fbm(sp * 3.0 + vec3<f32>(4.0), 4);
                var land = mix(b.col_a.rgb, b.col_b.rgb, smoothstep(0.4, 0.65, dry));
                land = mix(land, vec3<f32>(0.35, 0.3, 0.27), smoothstep(0.35, 0.6, hgt));
                land = mix(land, vec3<f32>(0.95), smoothstep(0.65, 0.8, hgt));
                s.albedo = land * (0.85 + 0.3 * fbm(sp * 25.0, 2));
            }
            let lat = abs(p.y) + (fbm(sp * 6.0, 3) - 0.5) * 0.15;
            s.albedo = mix(s.albedo, vec3<f32>(0.92, 0.95, 1.0), smoothstep(0.8, 0.86, lat));
            clouds = smoothstep(0.48, 0.72, fbm(sp * 3.2 + vec3<f32>(time * 0.012, 0.0, time * 0.006), 5)) * b.params.z;
        }
        case 3: { // desert
            let dark = smoothstep(0.45, 0.65, fbm(sp * 2.2, 4));
            s.albedo = mix(b.col_a.rgb, b.col_b.rgb, dark) * (0.85 + 0.3 * fbm(sp * 18.0, 2));
            let lat = abs(p.y) + (fbm(sp * 5.0, 3) - 0.5) * 0.1;
            s.albedo = mix(s.albedo, b.col_c.rgb, smoothstep(0.86, 0.9, lat));
        }
        case 4: { // cloudy (venus)
            let w = fbm(sp * 2.0 + vec3<f32>(time * 0.01, 0.0, 0.0), 4);
            let bands = 0.5 + 0.5 * sin((p.y + w * 0.4) * 9.0);
            s.albedo = mix(b.col_a.rgb, b.col_b.rgb, bands * 0.7 + w * 0.3);
        }
        case 5: { // gas giant
            let warp = fbm(sp * vec3<f32>(2.0, 6.0, 2.0) + vec3<f32>(time * 0.01, 0.0, 0.0), 4);
            let freq = 9.0 + fract(seed * 3.7) * 8.0;
            let band = 0.5 + 0.5 * sin((p.y + 0.12 * warp) * freq);
            let detail = fbm(vec3<f32>(sp.x * 5.0, sp.y * 26.0, sp.z * 5.0) + vec3<f32>(time * 0.02, 0.0, 0.0), 4);
            var c = mix(b.col_a.rgb, b.col_b.rgb, band);
            c = mix(c, b.col_c.rgb, smoothstep(0.45, 0.75, detail) * 0.6);
            s.albedo = c;
        }
        case 6: { // ice giant
            let w = fbm(sp * 2.0, 3);
            let band = 0.5 + 0.5 * sin(p.y * 7.0 + w * 1.5);
            s.albedo = mix(b.col_a.rgb, b.col_b.rgb, band * 0.35);
        }
        case 7: { // lava world
            let crack = ridged(sp * 4.0 + vec3<f32>(time * 0.01), 4);
            s.albedo = b.col_a.rgb * (0.7 + 0.6 * fbm(sp * 10.0, 2));
            let glow = smoothstep(0.55, 0.85, crack);
            s.emissive = heat_color(0.4 + glow * 0.4) * glow * 3.0;
        }
        case 8: { // ice world
            let lines = smoothstep(0.7, 0.85, ridged(sp * 5.0, 4));
            s.albedo = mix(b.col_a.rgb, b.col_b.rgb, lines * 0.8);
            s.spec = 0.3;
        }
        case 9: { // asteroid
            s.albedo = mix(b.col_a.rgb, b.col_b.rgb, fbm(sp * 6.0, 3)) * (0.8 + 0.3 * fbm(sp * 25.0, 2));
        }
        case 10: { // neutron star
            s.albedo = vec3<f32>(0.0);
            s.emissive = b.col_a.rgb * b.extra.x * (0.8 + 0.2 * pow(ndv, 0.5));
        }
        case 11: { // black hole: pure darkness
            return vec4<f32>(0.0, 0.0, 0.0, 1.0);
        }
        default: {}
    }

    // ---- painted materials ----
    let mat = i32(floor(pt.g * 255.0 / 32.0));
    let ms = clamp(pt.b * 1.2, 0.0, 1.0);
    if (mat > 0 && kind != K_STAR && kind != K_NEUTRON) {
        let mc = material_color(mat) * (0.85 + 0.3 * fbm(sp * 30.0, 2));
        s.albedo = mix(s.albedo, mc, ms);
        if (mat == 1) {
            s.spec = mix(s.spec, 1.0, ms);
        }
        if (mat == 2) {
            let f = fbm(sp * 12.0 + vec3<f32>(time * 0.05), 3);
            s.emissive = s.emissive + heat_color(0.55 + 0.4 * f) * 3.5 * ms * (0.6 + 0.6 * f);
        }
        if (mat == 3) {
            s.spec = mix(s.spec, 0.4, ms);
        }
    }

    // clouds over everything
    s.albedo = mix(s.albedo, vec3<f32>(0.95), clouds);
    s.spec = s.spec * (1.0 - clouds);
    let shade_n = normalize(mix(n, geo, clouds));

    // ---- lighting from all stars / flashes ----
    var color = s.albedo * g.params.w;
    var day = 0.0;
    let light_count = i32(g.params.x);
    for (var i = 0; i < light_count; i = i + 1) {
        let lp = g.light_pos[i].xyz;
        let to_l = lp - in.world;
        let dist = length(to_l);
        let l = to_l / dist;
        let atten = 80.0 / max(dist, 1.0);
        let lc = g.light_col[i].rgb * atten;
        let terminator = smoothstep(-0.08, 0.12, dot(geo, l));
        let diff = max(dot(shade_n, l), 0.0) * terminator;
        let h = normalize(l + v);
        let sp_term = pow(max(dot(shade_n, h), 0.0), 60.0) * s.spec * terminator;
        color = color + lc * (s.albedo * diff + vec3<f32>(sp_term * 0.6));
        day = day + clamp(dot(geo, l) + 0.35, 0.0, 1.0) * length(lc);
    }

    // atmosphere rim
    let rim = pow(1.0 - ndv, 3.5);
    color = color + b.atmo.rgb * b.atmo.w * rim * min(day, 3.0);

    // night-side city lights hint for earthlike? keep subtle: skip

    // heat from impacts, flash, selection highlight
    let heat = b.params.w;
    if (heat > 0.001) {
        let cracks = smoothstep(0.4, 0.8, ridged(sp * 5.0, 3));
        color = color + heat_color(clamp(heat, 0.0, 1.0) * 0.7) * heat * (0.12 + 2.2 * cracks);
    }
    color = color + vec3<f32>(1.0, 0.8, 0.55) * b.extra.y * 4.0;
    color = color + vec3<f32>(0.25, 0.6, 1.0) * b.extra.z * pow(1.0 - ndv, 6.0) * 0.12;
    color = color + s.emissive;
    return vec4<f32>(color, 1.0);
}
