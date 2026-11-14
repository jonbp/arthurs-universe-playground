use glam::{DVec3, Vec3};

/// Small, fast, deterministic PRNG (xorshift64*). Uses a `Cell` so it can be
/// drawn from with a shared reference (handy inside `self.emit(..., self.rng.f32())`).
#[derive(Clone)]
pub struct Rng(std::cell::Cell<u64>);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(std::cell::Cell::new(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1))
    }
    pub fn next_u64(&self) -> u64 {
        let mut x = self.0.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0.set(x);
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// Uniform in [0, 1)
    pub fn f64(&self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn f32(&self) -> f32 {
        self.f64() as f32
    }
    pub fn range(&self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f64()
    }
    pub fn rangef(&self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
    pub fn below(&self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
    /// Uniformly distributed unit vector.
    pub fn unit(&self) -> DVec3 {
        let z = self.range(-1.0, 1.0);
        let a = self.range(0.0, std::f64::consts::TAU);
        let r = (1.0 - z * z).sqrt();
        DVec3::new(r * a.cos(), z, r * a.sin())
    }
    /// Gaussian-ish via sum of uniforms.
    pub fn normal(&self) -> f64 {
        (self.f64() + self.f64() + self.f64() + self.f64() - 2.0) * 1.7320508
    }
}

/// Any vector perpendicular to `v` (unit length).
pub fn any_perpendicular(v: DVec3) -> DVec3 {
    let a = if v.y.abs() < 0.9 { DVec3::Y } else { DVec3::X };
    v.cross(a).normalize()
}


/// Approximate color of a blackbody at the given temperature (Kelvin), linear RGB.
pub fn blackbody(temp: f32) -> Vec3 {
    // Tanner Helland's approximation, then roughly linearised.
    let t = (temp / 100.0).clamp(10.0, 400.0);
    let r = if t <= 66.0 { 255.0 } else { 329.698727446 * (t - 60.0).powf(-0.1332047592) };
    let g = if t <= 66.0 {
        99.4708025861 * t.ln() - 161.1195681661
    } else {
        288.1221695283 * (t - 60.0).powf(-0.0755148492)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.5177312231 * (t - 10.0).ln() - 305.0447927307
    };
    let c = Vec3::new(r, g, b).clamp(Vec3::ZERO, Vec3::splat(255.0)) / 255.0;
    c.powf(2.2)
}
