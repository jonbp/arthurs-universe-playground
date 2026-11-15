//! What the game's numbers mean in the real world.
//!
//! Universes built to scale (the Solar System and most presets) use these units,
//! with G = 1:
//!
//! * one unit of length is half the Earth's radius (so the Earth's radius is 2),
//! * one unit of time is half an hour: at 1x speed, a second of play is 30 minutes,
//! * one unit of mass follows from those two and G = 1 (the Earth comes out at 39.95).
//!
//! Everything else is then simply the real figure converted: the Sun is 218 units
//! across its radius and 46,962 units away, the Earth takes 17,532 seconds of
//! simulated time to go round it, and it goes round at 16.8 units a second (29.8 km/s).
//!
//! A universe that is a model instead (the galaxies, the protoplanetary disk: see
//! `Universe::real_scale`) shows plain numbers, because its sizes and distances
//! are squeezed to make it watchable.

/// Kilometres in one unit of length: half the Earth's mean radius.
pub const KM: f64 = 6371.0 / 2.0;
/// Real seconds in one unit of simulated time.
pub const SECONDS: f64 = 1800.0;

/// Mass, from the body's real GM in km³/s² (the figure space agencies publish,
/// known far more exactly than the mass in kilograms).
pub const fn mass(gm_km3_s2: f64) -> f64 {
    gm_km3_s2 * SECONDS * SECONDS / (KM * KM * KM)
}

pub const fn km(km: f64) -> f64 {
    km / KM
}

pub const fn au(au: f64) -> f64 {
    au * AU
}

#[cfg(test)]
pub const fn days(days: f64) -> f64 {
    days * DAY
}

/// Spin rate (radians per unit of time) of something that turns once in `hours`.
pub fn spin_rate(hours: f64) -> f64 {
    std::f64::consts::TAU / (hours * 3600.0 / SECONDS)
}

pub const EARTH_MASS: f64 = mass(398_600.44);
pub const EARTH_RADIUS: f64 = 2.0;
pub const SUN_MASS: f64 = mass(132_712_440_018.0);
pub const SUN_RADIUS: f64 = km(695_700.0);
pub const SUN_TEMPERATURE: f32 = 5772.0;
pub const JUPITER_MASS: f64 = mass(126_686_531.9);
/// The astronomical unit: the average distance from the Earth to the Sun.
pub const AU: f64 = km(149_597_870.7);
pub const DAY: f64 = 86_400.0 / SECONDS;
pub const YEAR: f64 = 365.256_36 * DAY;
/// The speed of light.
pub const LIGHT_SPEED: f64 = 299_792.458 / (KM / SECONDS);
/// Kilometres per second in one unit of speed.
pub const KM_PER_S: f64 = KM / SECONDS;

/// "12,345": a whole number with commas.
pub fn commas(v: f64) -> String {
    let digits = format!("{:.0}", v.abs());
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if v < 0.0 && out != "0" { format!("-{out}") } else { out }
}

/// A number to about three figures, without a trailing ".0": 1, 1.5, 12.3, 123, 1,234.
fn short(v: f64) -> String {
    let a = v.abs();
    let s = if a >= 999.5 {
        return commas(v);
    } else if a >= 99.95 {
        format!("{v:.0}")
    } else if a >= 9.995 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    };
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

fn plural(v: &str, unit: &str) -> String {
    if v == "1" { format!("{v} {unit}") } else { format!("{v} {unit}s") }
}

/// A distance or a size: "384,400 km", "150 million km", "4.5 billion km".
pub fn fmt_distance(d: f64, real: bool) -> String {
    if !real {
        return if d.abs() >= 100.0 { commas(d) } else { format!("{d:.1}") };
    }
    let km = d * KM;
    let a = km.abs();
    if a < 9.95 {
        format!("{km:.1} km")
    } else if a < 999_500.0 {
        format!("{} km", commas(km))
    } else if a < 999.5e6 {
        format!("{} million km", short(km / 1e6))
    } else if a < 999.5e9 {
        format!("{} billion km", short(km / 1e9))
    } else {
        format!("{} light-years", short(km / 9.4607e12))
    }
}

/// A speed: "29.8 km/s".
pub fn fmt_speed(v: f64, real: bool) -> String {
    if !real {
        return format!("{v:.2}");
    }
    let kms = v * KM_PER_S;
    if kms.abs() < 0.0995 { format!("{:.0} m/s", kms * 1000.0) } else { format!("{} km/s", short(kms)) }
}

/// A length of time: "45 minutes", "27.3 days", "11.9 years".
pub fn fmt_duration(t: f64, real: bool) -> String {
    if !real {
        return if t < 120.0 {
            format!("{t:.0}s")
        } else if t < 7200.0 {
            format!("{:.1}m", t / 60.0)
        } else {
            format!("{:.1}h", t / 3600.0)
        };
    }
    let s = t * SECONDS;
    if s < 90.0 {
        plural(&short(s.round()), "second")
    } else if s < 5400.0 {
        plural(&short((s / 60.0).round()), "minute")
    } else if s < 2.0 * 86_400.0 {
        plural(&short(s / 3600.0), "hour")
    } else if t < 2.0 * YEAR {
        plural(&short(s / 86_400.0), "day")
    } else {
        plural(&short(t / YEAR), "year")
    }
}

/// The same, as short as it can be said, for the clock: "45 min", "27.3 d", "11.9 yr".
pub fn fmt_clock(t: f64, real: bool) -> String {
    if !real {
        return fmt_duration(t, false);
    }
    let s = t * SECONDS;
    if s < 5400.0 {
        format!("{:.0} min", s / 60.0)
    } else if s < 2.0 * 86_400.0 {
        format!("{:.1} h", s / 3600.0)
    } else if t < 2.0 * YEAR {
        format!("{:.1} d", s / 86_400.0)
    } else {
        format!("{:.2} yr", t / YEAR)
    }
}

/// How fast time is running, as what passes in one second of play:
/// "30 min/s", "1 day/s", "2 years/s". (Plain "6×" in a model universe.)
pub fn fmt_rate(time_scale: f64, real: bool) -> String {
    if !real {
        return if time_scale >= 9.95 {
            format!("{time_scale:.0}×")
        } else if time_scale >= 1.0 {
            let t = format!("{time_scale:.1}");
            format!("{}×", t.trim_end_matches(".0"))
        } else {
            format!("{time_scale:.2}×")
        };
    }
    let s = time_scale * SECONDS;
    // Two figures are plenty here, and "1 day/s" reads better than "1.02 days/s".
    let two = |v: f64| {
        let p = 10f64.powf(1.0 - v.abs().max(1e-9).log10().floor());
        short((v * p).round() / p)
    };
    if s < 59.5 {
        format!("{} sec/s", two(s))
    } else if s < 3570.0 {
        format!("{} min/s", two(s / 60.0))
    } else if s < 0.98 * 86_400.0 {
        format!("{}/s", plural(&two(s / 3600.0), "hour"))
    } else if s < 0.98 * 7.0 * 86_400.0 {
        format!("{}/s", plural(&two(s / 86_400.0), "day"))
    } else if s < 0.98 * 30.0 * 86_400.0 {
        format!("{}/s", plural(&two(s / (7.0 * 86_400.0)), "week"))
    } else if time_scale < 0.98 * YEAR {
        format!("{}/s", plural(&two(time_scale / (YEAR / 12.0)), "month"))
    } else {
        format!("{}/s", plural(&two(time_scale / YEAR), "year"))
    }
}

/// The speeds offered in the speed menu of a to-scale universe: (label, time scale).
pub const RATES: [(&str, f64); 6] =
    [("30 min", 1.0), ("1 day", DAY), ("1 week", 7.0 * DAY), ("1 month", YEAR / 12.0), ("1 year", YEAR), ("10 years", 10.0 * YEAR)];

/// A mass, counted in Earths (or Suns, for a star): "1 Earth", "318 Earths", "1.5 Suns".
pub fn fmt_mass(m: f64, real: bool) -> String {
    if !real {
        return short(m);
    }
    let suns = m / SUN_MASS;
    if suns >= 0.0795 {
        return plural(&short(suns), "Sun");
    }
    plural(&earths(m), "Earth")
}

/// A mass as a bare number of Earths, with enough figures for a small moon.
pub fn earths(m: f64) -> String {
    let e = m / EARTH_MASS;
    if e >= 0.0995 || e <= 0.0 {
        short(e)
    } else {
        // Two figures, however small: 0.0012, 0.000015.
        let places = (1.0 - e.log10().floor()).clamp(0.0, 12.0) as usize;
        format!("{e:.places$}")
    }
}

/// Reads back what [`earths`] and [`commas`] write (and whatever unit follows the number).
pub fn parse_number(s: &str) -> Option<f64> {
    let clean: String = s.trim().chars().filter(|c| *c != ',').collect();
    let end = clean.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))).unwrap_or(clean.len());
    clean[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_units_give_back_the_real_solar_system() {
        // Earth: radius 2 and surface gravity 9.8 m/s², by construction of the units.
        let g = EARTH_MASS / (EARTH_RADIUS * EARTH_RADIUS) * KM * 1000.0 / (SECONDS * SECONDS);
        assert!((g - 9.82).abs() < 0.01, "{g}");
        // A year is what Kepler says it should be for the Sun's mass at 1 AU.
        let year = std::f64::consts::TAU * (AU.powi(3) / (SUN_MASS + EARTH_MASS)).sqrt();
        assert!((year / YEAR - 1.0).abs() < 1e-4, "{year} vs {YEAR}");
        assert!((SUN_MASS / EARTH_MASS - 332_946.0).abs() < 1.0);
        assert!((SUN_RADIUS / EARTH_RADIUS - 109.2).abs() < 0.1);
        // The Earth goes round the Sun at 29.8 km/s.
        assert!(((SUN_MASS / AU).sqrt() * KM_PER_S - 29.78).abs() < 0.01);
    }

    #[test]
    fn numbers_read_well() {
        assert_eq!(fmt_distance(km(384_400.0), true), "384,400 km");
        assert_eq!(fmt_distance(AU, true), "150 million km");
        assert_eq!(fmt_distance(au(30.07), true), "4.5 billion km");
        assert_eq!(fmt_distance(12.34, false), "12.3");
        assert_eq!(fmt_speed(16.83, true), "29.8 km/s");
        assert_eq!(fmt_duration(YEAR, true), "365 days");
        assert_eq!(fmt_duration(days(27.32), true), "27.3 days");
        assert_eq!(fmt_duration(11.86 * YEAR, true), "11.9 years");
        assert_eq!(fmt_duration(3.0, true), "1.5 hours");
        assert_eq!(fmt_duration(90.0, false), "90s");
        assert_eq!(fmt_rate(1.0, true), "30 min/s");
        assert_eq!(fmt_rate(2.0, true), "1 hour/s");
        assert_eq!(fmt_rate(DAY, true), "1 day/s");
        assert_eq!(fmt_rate(7.0 * DAY, true), "1 week/s");
        assert_eq!(fmt_rate(YEAR / 12.0, true), "1 month/s");
        assert_eq!(fmt_rate(YEAR, true), "1 year/s");
        assert_eq!(fmt_rate(10.0 * YEAR, true), "10 years/s");
        assert_eq!(fmt_rate(0.2, true), "6 min/s");
        assert_eq!(fmt_rate(6.0, false), "6×");
        assert_eq!(fmt_mass(EARTH_MASS, true), "1 Earth");
        assert_eq!(fmt_mass(JUPITER_MASS, true), "318 Earths");
        assert_eq!(fmt_mass(SUN_MASS, true), "1 Sun");
        assert_eq!(earths(mass(4902.8)), "0.012");
        assert_eq!(parse_number("333,000 Earths"), Some(333_000.0));
        assert_eq!(parse_number(&earths(mass(0.0007087))).map(|v| v > 0.0), Some(true));
    }
}
