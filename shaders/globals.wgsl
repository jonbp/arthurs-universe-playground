// ---- Shared per-frame uniforms (must match `Globals` in src/gpu_types.rs) ----
struct Globals {
    view_proj: mat4x4<f32>,      // camera-relative view * projection (reverse-Z)
    inv_view_proj: mat4x4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    cam_fwd: vec4<f32>,
    // x = width px, y = height px, z = time (s), w = exposure
    screen: vec4<f32>,
    // x = light count, y = bloom strength, z = black hole count, w = ambient
    params: vec4<f32>,
    // xyz = camera-relative position, w = radius of light source
    light_pos: array<vec4<f32>, 8>,
    // rgb = color * intensity
    light_col: array<vec4<f32>, 8>,
    // xy = screen uv, z = horizon radius in uv-y units, w = 1 if visible
    black_holes: array<vec4<f32>, 8>,
    // rgb = atmosphere colour around the camera, a = how much it hides the stars
    sky: vec4<f32>,
    // xyz = "up" at the camera, w = sunlight reaching solid models (0 in a planet's shadow)
    sky_up: vec4<f32>,
};
