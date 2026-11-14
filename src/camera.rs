use glam::{DVec3, Mat4, Vec2, Vec3, Vec4};

/// Orbit camera. Rendering is camera-relative (the eye sits at the origin) so
/// f32 precision stays good even in huge universes.
pub struct Camera {
    pub target: DVec3,
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    pub desired_distance: f64,
    pub fov_y: f32,
    pub follow: Option<u64>,
    follow_offset: DVec3,
}

pub struct CameraFrame {
    pub eye: DVec3,
    pub view_proj: Mat4,
    pub inv_view_proj: Mat4,
    pub right: Vec3,
    pub up: Vec3,
    pub fwd: Vec3,
    pub tan_half: f32,
    pub size: Vec2,
}

impl Camera {
    pub fn new() -> Self {
        Camera {
            target: DVec3::ZERO,
            yaw: 0.6,
            pitch: 0.45,
            distance: 400.0,
            desired_distance: 400.0,
            fov_y: 50f32.to_radians(),
            follow: None,
            follow_offset: DVec3::ZERO,
        }
    }

    pub fn forward(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        -DVec3::new(cp * sy, sp, cp * cy)
    }

    pub fn eye(&self) -> DVec3 {
        self.target - self.forward() * self.distance
    }

    pub fn focus_on(&mut self, id: u64, pos: DVec3, radius: f64) {
        self.follow_offset = self.target - pos;
        self.follow = Some(id);
        self.desired_distance = (radius * 7.0).max(2.0);
    }

    pub fn rotate(&mut self, dx: f64, dy: f64) {
        self.yaw -= dx * 0.005;
        self.pitch = (self.pitch + dy * 0.005).clamp(-1.55, 1.55);
    }

    pub fn zoom(&mut self, amount: f64) {
        self.desired_distance = (self.desired_distance * (1.0 - amount * 0.12).clamp(0.3, 3.0)).clamp(0.3, 50000.0);
    }

    pub fn pan(&mut self, dx: f64, dy: f64) {
        let f = self.forward();
        let right = f.cross(DVec3::Y).try_normalize().unwrap_or(DVec3::X);
        let up = right.cross(f);
        let k = self.distance * 0.0015;
        self.target += (-right * dx + up * dy) * k;
        if self.follow.take().is_some() {
            self.follow_offset = DVec3::ZERO;
        }
    }

    /// Per-frame update: smooth zoom and body following.
    pub fn update(&mut self, dt: f64, follow_pos: Option<DVec3>) {
        let k = 1.0 - (-dt * 8.0).exp();
        self.distance += (self.desired_distance - self.distance) * k;
        match (self.follow, follow_pos) {
            (Some(_), Some(p)) => {
                self.follow_offset *= (-dt * 5.0).exp();
                self.target = p + self.follow_offset;
            }
            (Some(_), None) => self.follow = None,
            _ => {}
        }
    }

    pub fn frame(&self, width: f32, height: f32) -> CameraFrame {
        let near = (self.distance * 0.002).clamp(0.005, 2.0) as f32;
        CameraFrame::look(self.eye(), self.forward(), DVec3::Y, self.fov_y, near, width, height)
    }
}

impl CameraFrame {
    /// A view from `eye` along `forward`, with `up_hint` deciding which way is up
    /// (any vector not parallel to `forward`). Used by the orbit camera, the
    /// rocket's cockpit and chase cameras, and the astronaut's eyes.
    pub fn look(eye: DVec3, forward: DVec3, up_hint: DVec3, fov_y: f32, near: f32, width: f32, height: f32) -> CameraFrame {
        let fwd = forward.as_vec3().try_normalize().unwrap_or(Vec3::NEG_Z);
        let hint = up_hint.as_vec3();
        let right = fwd
            .cross(hint)
            .try_normalize()
            .or_else(|| fwd.cross(Vec3::Y).try_normalize())
            .unwrap_or_else(|| fwd.cross(Vec3::X).normalize());
        let up = right.cross(fwd);
        // Right-handed view matrix with the eye at the origin.
        let view = Mat4::from_cols(
            Vec4::new(right.x, up.x, -fwd.x, 0.0),
            Vec4::new(right.y, up.y, -fwd.y, 0.0),
            Vec4::new(right.z, up.z, -fwd.z, 0.0),
            Vec4::W,
        );
        let aspect = width / height.max(1.0);
        // Infinite reverse-Z perspective (depth 1 at the near plane, 0 at infinity),
        // with Y flipped because Vulkan clip space has +y pointing down.
        let f = 1.0 / (fov_y * 0.5).tan();
        let proj = Mat4::from_cols(
            Vec4::new(f / aspect, 0.0, 0.0, 0.0),
            Vec4::new(0.0, -f, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 0.0, -1.0),
            Vec4::new(0.0, 0.0, near, 0.0),
        );
        let view_proj = proj * view;
        CameraFrame { eye, view_proj, inv_view_proj: view_proj.inverse(), right, up, fwd, tan_half: (fov_y * 0.5).tan(), size: Vec2::new(width, height) }
    }

    /// World-space ray direction through a pixel (physical pixels, y down).
    pub fn ray(&self, px: Vec2) -> DVec3 {
        let x = (px.x / self.size.x) * 2.0 - 1.0;
        let y = (px.y / self.size.y) * 2.0 - 1.0;
        let aspect = self.size.x / self.size.y;
        let d = self.fwd + self.right * (x * self.tan_half * aspect) - self.up * (y * self.tan_half);
        d.normalize().as_dvec3()
    }

    /// Project a world position to physical pixel coordinates (None if behind).
    pub fn project(&self, p: DVec3) -> Option<Vec2> {
        let rel = (p - self.eye).as_vec3();
        let clip = self.view_proj * Vec4::new(rel.x, rel.y, rel.z, 1.0);
        if clip.w <= 1e-4 {
            return None;
        }
        let ndc = Vec2::new(clip.x, clip.y) / clip.w;
        Some(Vec2::new((ndc.x + 1.0) * 0.5 * self.size.x, (ndc.y + 1.0) * 0.5 * self.size.y))
    }

    /// Apparent radius in pixels of a sphere.
    pub fn screen_radius(&self, p: DVec3, r: f64) -> f32 {
        let depth = (p - self.eye).as_vec3().dot(self.fwd).max(1e-4);
        (r as f32 / depth) / self.tan_half * self.size.y * 0.5
    }

    pub fn rel(&self, p: DVec3) -> Vec3 {
        (p - self.eye).as_vec3()
    }
}

pub fn ray_sphere(origin: DVec3, dir: DVec3, center: DVec3, radius: f64) -> Option<f64> {
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.length_squared() - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let t = -b - s;
    if t > 0.0 {
        Some(t)
    } else if -b + s > 0.0 {
        Some(-b + s)
    } else {
        None
    }
}
