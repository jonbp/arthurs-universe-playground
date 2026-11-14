// ---- Deep space: stars, the Milky Way, glowing gas and far-off galaxies ----
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

// Fixed rotations (each a turn about a different slanted axis).
const TURN_A: mat3x3<f32> = mat3x3<f32>(vec3<f32>(0.36, 0.48, -0.8), vec3<f32>(-0.8, 0.6, 0.0), vec3<f32>(0.48, 0.64, 0.6));
const TURN_B: mat3x3<f32> = mat3x3<f32>(vec3<f32>(0.6, 0.0, 0.8), vec3<f32>(0.64, 0.6, -0.48), vec3<f32>(-0.48, 0.8, 0.36));
const TURN_C: mat3x3<f32> = mat3x3<f32>(vec3<f32>(0.0, 0.8, 0.6), vec3<f32>(-0.6, 0.48, -0.64), vec3<f32>(-0.8, -0.36, 0.48));

// One star at most per cell of a grid laid over the sky. `density` is the share of
// cells that hold one; `rarity` is how strongly brightness is skewed towards faint
// stars (as in the real sky: a handful of bright ones among very many dim ones).
fn star_layer(dir: vec3<f32>, scale: f32, density: f32, rarity: f32, gain: f32, px: f32) -> vec3<f32> {
    let q = dir * scale;
    let cell = floor(q);
    let f = fract(q);
    // Most cells are empty: find that out with one cheap hash before doing any more.
    if (hash3(cell) > density) {
        return vec3<f32>(0.0);
    }
    let h = hash33(cell);
    let pos = vec3<f32>(0.18) + h * 0.64;
    let d = length(f - pos);
    // Energy-conserving anti-aliasing: a star never gets narrower than ~1px;
    // when it would be smaller it is spread over a pixel and dimmed instead,
    // so it fades smoothly rather than twinkling/aliasing as the camera moves.
    let w0 = 0.022;
    let w = max(w0, px * scale * 0.75);
    let energy = (w0 / w) * (w0 / w);
    let bright = pow(hash3(cell + vec3<f32>(3.0)), rarity) * gain + 0.06;
    // A tight core, plus a soft halo that only shows on the brightest.
    let core = exp(-(d * d) / (w * w)) + 0.03 * exp(-(d * d) / (w * w * 10.0)) * min(bright, 2.0);
    // Colour by temperature: mostly white, some orange and red, a few blue.
    let temp = hash3(cell + vec3<f32>(7.0));
    let warm = mix(vec3<f32>(1.0, 0.55, 0.35), vec3<f32>(1.0, 0.86, 0.7), smoothstep(0.0, 0.35, temp));
    let tint = mix(warm, vec3<f32>(0.7, 0.82, 1.0), smoothstep(0.55, 1.0, temp));
    return tint * core * bright * energy;
}

// Far-off galaxies: small soft ovals with a bright middle, scattered thinly.
fn galaxies(dir: vec3<f32>) -> vec3<f32> {
    let scale = 7.0;
    let q = dir * scale;
    let cell = floor(q);
    if (hash3(cell + vec3<f32>(51.0)) > 0.3) {
        return vec3<f32>(0.0);
    }
    let h = hash33(cell + vec3<f32>(51.0));
    let v = fract(q) - (vec3<f32>(0.3) + h * 0.4);
    // Seen at a random slant: long along one axis, squashed across it.
    let axis = normalize(hash33(cell + vec3<f32>(77.0)) - vec3<f32>(0.5));
    let size = mix(0.018, 0.06, h.y * h.y * 9.0 * h.x);
    let squash = mix(0.25, 0.9, h.z);
    let along = dot(v, axis);
    let across2 = max(dot(v, v) - along * along, 0.0);
    let r2 = (along * along + across2 / (squash * squash)) / (size * size);
    let glow = exp(-r2) * 0.035 + exp(-r2 * 14.0) * 0.09;
    let tint = mix(vec3<f32>(1.0, 0.85, 0.7), vec3<f32>(0.7, 0.8, 1.0), hash3(cell + vec3<f32>(9.0)));
    return tint * glow;
}

@fragment
fn fs_main(in: SkyOut) -> @location(0) vec4<f32> {
    // Reverse-Z: far plane is z = 0; point at depth ~1 is near.
    let world_h = g.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let dir = normalize(world_h.xyz / world_h.w);
    let px = 2.0 * g.cam_fwd.w / g.screen.y;
    let star_amount = g.cam_right.w;

    // ---- The Milky Way: the disc of our own galaxy seen edge-on from inside ----
    // A band right round the sky, tilted so it crosses the usual views at a slant,
    // brightest and fattest towards the galaxy's middle.
    let pole = normalize(vec3<f32>(0.62, 0.55, -0.56));
    let middle = normalize(cross(pole, vec3<f32>(0.0, 1.0, 0.0)));
    let lat = dot(dir, pole);
    let toward = dot(normalize(dir - pole * lat), middle);      // 1 looking at the middle, -1 away
    let bulge = smoothstep(0.55, 1.0, toward);
    let width = mix(0.13, 0.27, bulge);
    let band = exp(-(lat * lat) / (width * width));
    // Clumpy star clouds, crossed by dark lanes of dust that hug the middle of the
    // band, and glowing gas: pink hydrogen and a little blue, in wisps. All of it
    // lies along the band, so the noise is only worked out there (it is the costly
    // part of this shader, and most of the sky is plain black between the stars).
    var thick = 0.0;
    var lanes = 0.0;
    var gas = vec3<f32>(0.0);
    let near_band = exp(-(lat * lat) / 0.09);
    if (near_band > 0.01) {
        let clouds = fbm(dir * 3.2 + vec3<f32>(11.0), 3);
        let dust = fbm(dir * vec3<f32>(6.5, 7.5, 6.5) + vec3<f32>(40.0), 3);
        lanes = smoothstep(0.40, 0.60, dust) * exp(-(lat * lat) / (width * width * 0.3));
        // (Faded out towards where the noise stops, so no edge shows.)
        let fade = smoothstep(0.01, 0.08, near_band);
        thick = band * (0.5 + 0.5 * pow(toward * 0.5 + 0.5, 2.0)) * (0.3 + 1.2 * clouds) * (1.0 - 0.85 * lanes) * fade;
        // One noise gives both colours: pink where it is high, blue where it is low.
        let tone = fbm(dir * 3.4 + vec3<f32>(30.0), 3);
        let pink = smoothstep(0.60, 0.85, tone);
        let blue = smoothstep(0.30, 0.12, tone);
        let wisps = 0.15 + 1.7 * dust;
        gas = (vec3<f32>(0.06, 0.014, 0.028) * pink + vec3<f32>(0.009, 0.02, 0.04) * blue) * wisps * near_band * (1.0 - 0.7 * lanes);
    }

    var col = vec3<f32>(0.0);
    // Stars, from a few bright ones to a fine dust of faint ones. There are many
    // more of the faint ones towards the Milky Way, and fewer behind its dust.
    let crowd = thick * 2.2;
    // (Each layer's grid is turned a different way, so the grids never line up
    // with each other or with the screen and no pattern shows through.)
    col = col + star_layer(dir, 26.0, 0.10, 3.0, 22.0, px);
    col = col + star_layer(TURN_A * dir, 55.0, 0.20, 4.0, 14.0, px);
    col = col + star_layer(TURN_B * dir, 115.0, min(0.16 + 0.45 * crowd, 0.6), 3.0, 9.0, px);
    col = col + star_layer(TURN_C * dir, 190.0, min(0.14 + 0.5 * crowd, 0.6), 2.0, 7.0, px);
    col = col * star_amount * (1.0 - 0.6 * lanes * band);

    // The glow of all the stars too far away to see one by one: warm in the
    // middle of the galaxy, cooler and bluer along the arms.
    let milky = mix(vec3<f32>(0.50, 0.58, 0.80), vec3<f32>(0.95, 0.84, 0.70), bulge) * thick * 0.05;

    // Other galaxies show where our own doesn't hide them.
    let beyond = galaxies(dir) * (1.0 - min(band * 1.5, 1.0));

    col = col + (milky + gas + beyond) * (0.35 + 0.65 * min(star_amount, 1.5));

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
