// ---- Scene bindings (set 0) ----
struct GpuBody {
    pos_radius: vec4<f32>,  // camera-relative position, radius
    rot: vec4<f32>,         // local -> world quaternion
    col_a: vec4<f32>,       // rgb, w = kind
    col_b: vec4<f32>,       // rgb, w = seed
    col_c: vec4<f32>,       // rgb, w = paint layer
    params: vec4<f32>,      // x terrain amp, y sea level, z clouds, w heat
    atmo: vec4<f32>,        // rgb, w strength
    extra: vec4<f32>,       // x emissive, y flash, z highlight, w unused
};

struct GpuRing {
    center: vec4<f32>,      // camera-relative center, w = planet radius (for shadows)
    normal: vec4<f32>,      // plane normal, w = inner radius
    color: vec4<f32>,       // rgba
    params: vec4<f32>,      // x outer radius, y style (0 ring, 1 accretion disk), z seed, w intensity
};

struct GpuParticle {
    pos_size: vec4<f32>,    // camera-relative position, size
    color: vec4<f32>,       // rgb premultiplied, w = shape (0 soft dot, 1 wide glow)
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var<storage, read> bodies: array<GpuBody>;
@group(0) @binding(2) var<storage, read> rings: array<GpuRing>;
@group(0) @binding(3) var<storage, read> particles: array<GpuParticle>;
@group(0) @binding(4) var paint_tex: texture_2d_array<f32>;
@group(0) @binding(5) var paint_samp: sampler;

const PI: f32 = 3.14159265;
const TAU: f32 = 6.2831853;

fn quat_rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

fn quat_conj(q: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(-q.xyz, q.w);
}

// ---- Noise ----
fn hash3(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.3183099 + vec3<f32>(0.71, 0.113, 0.419));
    q = q * 17.0;
    return fract(q.x * q.y * q.z * (q.x + q.y + q.z));
}

fn hash33(p: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(hash3(p), hash3(p + vec3<f32>(17.3, 5.1, 9.7)), hash3(p + vec3<f32>(3.7, 41.2, 23.9)));
}

// ---- Lattice hashes for terrain ----
// The float hashes above are chaotic: one differently-rounded multiply-add (GPUs
// are free to fuse them) gives a completely different value. That is fine for
// stars, but terrain is also evaluated on the CPU (src/terrain.rs) so rockets can
// land on it and people can walk on it, and there both sides must agree. These
// hash the integer cell coordinates with integer maths, which is exact everywhere.
fn mix32(x: u32) -> u32 {
    var h = x;
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    return h ^ (h >> 16u);
}

fn cell_hash(cell: vec3<f32>) -> u32 {
    let c = bitcast<vec3<u32>>(vec3<i32>(cell));
    return mix32(c.x * 0x8da6b343u ^ c.y * 0xd8163841u ^ c.z * 0xcb1ab31fu);
}

fn unit_float(h: u32) -> f32 {
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

// One value in [0, 1) per integer cell.
fn lattice(cell: vec3<f32>) -> f32 {
    return unit_float(cell_hash(cell));
}

// Three independent values in [0, 1) per integer cell.
fn lattice3(cell: vec3<f32>) -> vec3<f32> {
    let a = cell_hash(cell);
    let b = mix32(a ^ 0x9e3779b9u);
    let c = mix32(b ^ 0x85ebca6bu);
    return vec3<f32>(unit_float(a), unit_float(b), unit_float(c));
}

fn noise3(x: vec3<f32>) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let u = f * f * (3.0 - 2.0 * f);
    let a = lattice(i + vec3<f32>(0.0, 0.0, 0.0));
    let b = lattice(i + vec3<f32>(1.0, 0.0, 0.0));
    let c = lattice(i + vec3<f32>(0.0, 1.0, 0.0));
    let d = lattice(i + vec3<f32>(1.0, 1.0, 0.0));
    let e = lattice(i + vec3<f32>(0.0, 0.0, 1.0));
    let f2 = lattice(i + vec3<f32>(1.0, 0.0, 1.0));
    let g2 = lattice(i + vec3<f32>(0.0, 1.0, 1.0));
    let h = lattice(i + vec3<f32>(1.0, 1.0, 1.0));
    return mix(
        mix(mix(a, b, u.x), mix(c, d, u.x), u.y),
        mix(mix(e, f2, u.x), mix(g2, h, u.x), u.y),
        u.z
    );
}

fn fbm(p: vec3<f32>, octaves: i32) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i = 0; i < octaves; i = i + 1) {
        sum = sum + amp * noise3(q);
        q = q * 2.03 + vec3<f32>(1.7, 9.2, 3.1);
        amp = amp * 0.5;
    }
    return sum;
}

fn ridged(p: vec3<f32>, octaves: i32) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i = 0; i < octaves; i = i + 1) {
        let n = 1.0 - abs(noise3(q) * 2.0 - 1.0);
        sum = sum + amp * n * n;
        q = q * 2.1 + vec3<f32>(5.3, 1.3, 7.7);
        amp = amp * 0.5;
    }
    return sum;
}

// Cellular craters: returns height contribution (negative bowl + raised rim).
fn craters(p: vec3<f32>, scale: f32, seed: f32) -> f32 {
    let q = p * scale + vec3<f32>(seed * 13.0);
    let i = floor(q);
    let f = fract(q);
    var h = 0.0;
    for (var z = -1; z <= 1; z = z + 1) {
        for (var y = -1; y <= 1; y = y + 1) {
            for (var x = -1; x <= 1; x = x + 1) {
                let o = vec3<f32>(f32(x), f32(y), f32(z));
                let rnd = lattice3(i + o);
                let r = 0.18 + 0.3 * rnd.z * rnd.z;
                if (rnd.x > 0.55) {
                    continue;
                }
                let d = length(o + rnd - f) / r;
                if (d < 1.4) {
                    let bowl = (d * d - 1.0) * 0.6;
                    let rim = exp(-pow((d - 1.0) / 0.18, 2.0)) * 0.35;
                    h = h + select(rim * smoothstep(1.4, 1.0, d), bowl + rim, d < 1.0) * r;
                }
            }
        }
    }
    return h;
}

// Equirectangular mapping (must match src/paint.rs)
fn sphere_uv(p: vec3<f32>) -> vec2<f32> {
    let u = atan2(p.z, p.x) / TAU + 0.5;
    let v = acos(clamp(p.y, -1.0, 1.0)) / PI;
    return vec2<f32>(u, v);
}

// Approximate blackbody color for a normalized heat value 0..1
fn heat_color(t: f32) -> vec3<f32> {
    let c1 = vec3<f32>(0.5, 0.02, 0.0);
    let c2 = vec3<f32>(1.0, 0.35, 0.05);
    let c3 = vec3<f32>(1.0, 0.85, 0.5);
    return select(mix(c2, c3, (t - 0.5) * 2.0), mix(c1, c2, t * 2.0), t < 0.5);
}

// Fully transformed position of a quad corner for camera-facing billboards
fn quad_corner(vi: u32) -> vec2<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0)
    );
    return corners[vi];
}
