//! What it is like on a world: temperature, gravity, air, water.
//!
//! A simple physical model, calibrated so the Solar System preset lands near
//! the real numbers (Earth 15 °C, Venus ~460 °C, Mars about -60 °C, the Moon's
//! huge day/night swing, ...). Distances are measured against the preset's
//! Earth orbit and brightness against its Sun.

use glam::DVec3;

use crate::body::{Body, BodyKind};
use crate::universe::Universe;

/// One "astronomical unit": Earth's orbit radius in the Solar System preset.
const AU: f64 = 430.0;
/// The preset Sun's radius and temperature (luminosity 1).
const SUN_RADIUS: f64 = 16.0;
const SUN_TEMP: f64 = 5800.0;
/// Surface gravity of the preset Earth (mass 40, radius 2, G = 1).
const EARTH_GRAVITY: f64 = 10.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Good,
    Caution,
    Danger,
}

#[derive(Clone, Debug)]
pub struct Conditions {
    /// Average surface (or cloud-top) temperature in kelvin.
    pub temp_k: f64,
    pub day_k: f64,
    pub night_k: f64,
    /// Surface gravity relative to Earth.
    pub gravity_g: f64,
    /// Air pressure in Earth atmospheres.
    pub pressure_atm: f64,
    /// Starlight compared with what Earth gets from the Sun.
    pub sunlight: f64,
    /// Length of a day in simulated seconds (None if it barely spins).
    pub day_length: Option<f64>,
    pub escape_speed: f64,
    pub air: &'static str,
    pub water: &'static str,
    pub verdict: &'static str,
    pub level: Level,
    /// How much the temperature follows the sun (0 = thick air evens it out, 1 = none).
    swing: f64,
    solid: bool,
}

/// Physical brightness of a star relative to the preset Sun.
fn star_power(b: &Body) -> f64 {
    match b.kind {
        BodyKind::Star => (b.radius / SUN_RADIUS).powi(2) * (b.temperature as f64 / SUN_TEMP).powi(4),
        BodyKind::NeutronStar => 0.002,
        _ => 0.0,
    }
}

/// Starlight arriving at `pos` (Earth = 1), ignoring `exclude`.
pub fn sunlight_at(u: &Universe, pos: DVec3, exclude: Option<u64>) -> f64 {
    u.bodies
        .iter()
        .filter(|s| Some(s.id) != exclude && s.kind.is_luminous())
        .map(|s| star_power(s) / ((s.pos - pos).length_squared() / (AU * AU)).max(1e-9))
        .sum()
}

/// Direction to the star that lights `pos` the most.
pub fn sun_direction(u: &Universe, pos: DVec3, exclude: Option<u64>) -> Option<DVec3> {
    u.bodies
        .iter()
        .filter(|s| Some(s.id) != exclude && s.kind.is_luminous())
        .max_by(|a, b| {
            let fa = star_power(a) / (a.pos - pos).length_squared().max(1e-9);
            let fb = star_power(b) / (b.pos - pos).length_squared().max(1e-9);
            fa.total_cmp(&fb)
        })
        .and_then(|s| (s.pos - pos).try_normalize())
}

fn albedo(kind: BodyKind) -> f64 {
    match kind {
        BodyKind::Rocky | BodyKind::Asteroid => 0.12,
        BodyKind::Earthlike | BodyKind::IceGiant => 0.30,
        BodyKind::Desert => 0.25,
        BodyKind::Cloudy => 0.75,
        BodyKind::GasGiant => 0.5,
        BodyKind::Lava => 0.1,
        BodyKind::Ice => 0.6,
        _ => 0.0,
    }
}

/// Air pressure at the surface, in Earth atmospheres.
fn pressure(b: &Body, gravity_g: f64) -> f64 {
    let a = b.surface.atmo_strength.max(0.0) as f64;
    match b.kind {
        BodyKind::Earthlike => (a / 0.9) * gravity_g,
        BodyKind::Desert => 0.006 * (a / 0.25) * (gravity_g / 0.22),
        BodyKind::Cloudy => 92.0 * (b.mass / 32.0).powf(0.6) * (a / 0.6),
        BodyKind::Lava => 0.3 * (a / 0.4) * gravity_g,
        BodyKind::GasGiant | BodyKind::IceGiant => 1.0,
        // Airless kinds only have air if someone gives them an atmosphere.
        BodyKind::Rocky | BodyKind::Ice | BodyKind::Asteroid => 0.5 * a * gravity_g,
        _ => 0.0,
    }
}

pub fn conditions(u: &Universe, b: &Body) -> Conditions {
    let gravity = u.g * b.mass / (b.radius * b.radius).max(1e-9);
    let gravity_g = gravity / EARTH_GRAVITY;
    let escape_speed = (2.0 * u.g * b.mass / b.radius.max(1e-9)).sqrt();
    let day_length = (b.spin_rate.abs() > 1e-4).then(|| std::f64::consts::TAU / b.spin_rate.abs());

    if b.kind.is_stellar() {
        let (temp_k, air, verdict) = match b.kind {
            BodyKind::Star => (b.temperature as f64, "Glowing plasma", "A ball of fire: nothing can land here"),
            BodyKind::NeutronStar => (600_000.0, "A thin skin of iron plasma", "Gravity would flatten anything"),
            _ => (0.0, "Nothing escapes, not even light", "Do not get close"),
        };
        return Conditions {
            temp_k,
            day_k: temp_k,
            night_k: temp_k,
            gravity_g,
            pressure_atm: 0.0,
            sunlight: 0.0,
            day_length,
            escape_speed,
            air,
            water: "None",
            verdict,
            level: Level::Danger,
            swing: 0.0,
            solid: false,
        };
    }

    let solid = crate::terrain::has_solid_surface(b.kind);
    let sunlight = sunlight_at(u, b.pos, Some(b.id));
    let pressure_atm = pressure(b, gravity_g);

    // Balance of absorbed starlight and radiated heat, plus heat from inside.
    let equilibrium = 255.0 * (sunlight * (1.0 - albedo(b.kind)) / 0.7).powf(0.25);
    let internal: f64 = match b.kind {
        BodyKind::Lava => 700.0,
        BodyKind::GasGiant => 90.0,
        BodyKind::IceGiant => 50.0,
        _ => 0.0,
    };
    let mut temp_k = (equilibrium.powi(4) + internal.powi(4) + 3.0f64.powi(4)).powf(0.25);
    // Greenhouse warming grows with the amount of air.
    if solid {
        temp_k *= 1.0 + 0.13 * pressure_atm.powf(0.63);
    }
    // Fresh impacts leave the crust molten.
    temp_k += b.heat.clamp(0.0, 1.0) as f64 * 1500.0;

    let swing = if solid { 1.0 / (1.0 + 20.0 * pressure_atm) } else { 0.0 };
    let day_k = temp_k * (1.0 + 0.4 * swing);
    let night_k = temp_k * (1.0 - 0.45 * swing);

    let air = if !solid {
        if b.kind == BodyKind::GasGiant { "Hydrogen and helium, all the way down" } else { "Hydrogen, helium and methane" }
    } else if pressure_atm < 1e-4 {
        "None: open to space"
    } else {
        match b.kind {
            BodyKind::Earthlike if pressure_atm < 0.4 => "Nitrogen and oxygen, but very thin",
            BodyKind::Earthlike if pressure_atm > 3.0 => "Nitrogen and oxygen, heavy and dense",
            BodyKind::Earthlike => "Nitrogen and oxygen",
            BodyKind::Desert => "Thin carbon dioxide",
            BodyKind::Cloudy if pressure_atm > 10.0 => "Crushing carbon dioxide with acid clouds",
            BodyKind::Cloudy => "Hazy nitrogen and methane",
            BodyKind::Lava => "Sulphur and volcanic gas",
            _ => "A thin, unbreathable haze",
        }
    };

    // Water boils at a higher temperature under more pressure.
    let boiling = 373.0 * (1.0 + 0.075 * pressure_atm.max(1e-6).ln()).max(0.6);
    let water = if !solid {
        "Ice crystals in the clouds"
    } else if temp_k < 273.0 {
        "Frozen solid"
    } else if pressure_atm < 0.006 {
        "Boils away into space"
    } else if temp_k < boiling {
        "Liquid"
    } else {
        "Steam"
    };

    let breathable = b.kind == BodyKind::Earthlike && (0.4..=3.0).contains(&pressure_atm);
    let (verdict, level) = if !solid {
        ("No ground to stand on", Level::Danger)
    } else if temp_k > 600.0 {
        ("Hot enough to melt lead", Level::Danger)
    } else if pressure_atm > 20.0 {
        ("The air would crush you", Level::Danger)
    } else if temp_k > 335.0 {
        ("Far too hot", Level::Danger)
    } else if temp_k < 120.0 {
        ("Cold enough to freeze air", Level::Danger)
    } else if breathable && (265.0..=315.0).contains(&temp_k) {
        ("Just right for people", Level::Good)
    } else if breathable {
        (if temp_k < 265.0 { "Breathable but freezing" } else { "Breathable but sweltering" }, Level::Caution)
    } else if pressure_atm < 1e-4 {
        ("Spacesuit needed: no air", Level::Caution)
    } else if temp_k < 230.0 {
        ("Spacesuit needed: bitterly cold", Level::Caution)
    } else {
        ("Spacesuit needed: can't breathe the air", Level::Caution)
    };

    Conditions {
        temp_k,
        day_k,
        night_k,
        gravity_g,
        pressure_atm,
        sunlight,
        day_length,
        escape_speed,
        air,
        water,
        verdict,
        level,
        swing,
        solid,
    }
}

impl Conditions {
    /// Temperature at one spot. `sun_height` is the sine of the sun's angle above
    /// the horizon (-1 midnight .. 1 overhead); `latitude_sin` is 0 at the equator, ±1 at the poles.
    pub fn local_temperature(&self, sun_height: f64, latitude_sin: f64) -> f64 {
        if !self.solid {
            return self.temp_k;
        }
        let sun = if sun_height > 0.0 { 0.4 * sun_height } else { -0.45 * (-sun_height * 3.0).min(1.0) };
        // Thick air carries heat around the day/night line but poles still stay colder.
        let poles = -(1.0 - self.swing) * 0.12 * (latitude_sin * latitude_sin - 0.33);
        self.temp_k * (1.0 + self.swing * sun + poles)
    }
}

pub fn celsius(kelvin: f64) -> f64 {
    kelvin - 273.15
}

/// "15 °C", "-180 °C", "5,500 °C".
pub fn fmt_temp(kelvin: f64) -> String {
    let c = celsius(kelvin).round();
    if c.abs() >= 10_000.0 {
        format!("{:.0},{:03.0} °C", (c / 1000.0).trunc(), (c.abs() % 1000.0))
    } else if c.abs() >= 1000.0 {
        format!("{}{:.0},{:03.0} °C", if c < 0.0 { "-" } else { "" }, (c.abs() / 1000.0).trunc(), c.abs() % 1000.0)
    } else {
        format!("{c:.0} °C")
    }
}

pub fn fmt_pressure(atm: f64) -> String {
    if atm < 1e-4 {
        "None".into()
    } else if atm < 0.1 {
        format!("{atm:.3} atm")
    } else if atm < 10.0 {
        format!("{atm:.2} atm")
    } else {
        format!("{atm:.0} atm")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::{self, Preset};

    fn solar() -> Universe {
        let mut u = Universe::new("", 1);
        presets::build(&mut u, Preset::SolarSystem);
        u
    }

    fn of(u: &Universe, name: &str) -> Conditions {
        conditions(u, u.bodies.iter().find(|b| b.name == name).unwrap())
    }

    #[test]
    fn solar_system_temperatures_are_plausible() {
        let u = solar();
        let c = |n: &str| celsius(of(&u, n).temp_k);
        assert!((5.0..=25.0).contains(&c("Earth")), "Earth {}", c("Earth"));
        assert!((380.0..=520.0).contains(&c("Venus")), "Venus {}", c("Venus"));
        assert!((-80.0..=-40.0).contains(&c("Mars")), "Mars {}", c("Mars"));
        assert!((-190.0..=-120.0).contains(&c("Jupiter")), "Jupiter {}", c("Jupiter"));
        assert!(c("Neptune") < -200.0, "Neptune {}", c("Neptune"));
        assert!(c("Pluto") < -210.0, "Pluto {}", c("Pluto"));
        // Hotter the closer you are to the Sun, for airless rocks.
        assert!(c("Mercury") > c("Moon"));
    }

    #[test]
    fn airless_worlds_swing_between_day_and_night() {
        let u = solar();
        let moon = of(&u, "Moon");
        assert!(celsius(moon.day_k) > 80.0 && celsius(moon.night_k) < -100.0, "{moon:?}");
        let earth = of(&u, "Earth");
        assert!(earth.day_k - earth.night_k < 20.0, "Earth's air evens out day and night");
        assert!(moon.local_temperature(1.0, 0.0) > moon.local_temperature(-1.0, 0.0) + 150.0);
        assert!(earth.local_temperature(0.5, 0.0) > earth.local_temperature(0.5, 1.0), "poles are colder");
    }

    #[test]
    fn gravity_air_and_verdicts() {
        let u = solar();
        let earth = of(&u, "Earth");
        assert!((earth.gravity_g - 1.0).abs() < 1e-9);
        assert!((earth.pressure_atm - 1.0).abs() < 0.05);
        assert_eq!(earth.level, Level::Good);
        assert_eq!(earth.water, "Liquid");
        let moon = of(&u, "Moon");
        assert!((0.1..0.25).contains(&moon.gravity_g));
        assert_eq!(moon.pressure_atm, 0.0);
        assert_eq!(moon.level, Level::Caution);
        assert_eq!(of(&u, "Venus").level, Level::Danger);
        assert!(of(&u, "Venus").pressure_atm > 50.0);
        assert!((0.5..3.0).contains(&of(&u, "Titan").pressure_atm));
        assert_eq!(of(&u, "Jupiter").verdict, "No ground to stand on");
        assert_eq!(of(&u, "Sun").level, Level::Danger);
        assert!(of(&u, "Mars").water == "Frozen solid");
    }

    #[test]
    fn no_star_means_deep_cold() {
        let mut u = Universe::new("", 1);
        let b = u.new_body("Rogue", BodyKind::Rocky, DVec3::ZERO, DVec3::ZERO);
        let id = u.add(b);
        let c = conditions(&u, u.get(id).unwrap());
        assert!(c.temp_k < 10.0);
        assert_eq!(c.sunlight, 0.0);
    }

    #[test]
    fn temperature_formatting() {
        assert_eq!(fmt_temp(288.15), "15 °C");
        assert_eq!(fmt_temp(93.15), "-180 °C");
        assert_eq!(fmt_temp(5800.0), "5,527 °C");
        assert_eq!(fmt_temp(600_000.0), "599,727 °C");
    }
}
