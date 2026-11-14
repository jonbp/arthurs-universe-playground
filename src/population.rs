//! Who lives where. Every world with solid ground has a population that follows
//! its conditions (see `conditions.rs`): people multiply where it is "just right",
//! a few can hang on in bases where a spacesuit is needed, and they move away from
//! anywhere that turns deadly. Settlers set out from a well-populated world to any
//! empty one that is just right.
//!
//! Nobody is ever hurt: when a world is smashed, swallowed or scorched its people
//! escape in their rockets (`Universe::evacuate`), and the event log says so.

use crate::body::Body;
use crate::conditions::{self, Conditions, Level};
use crate::universe::Universe;

/// How many people a just-right world the size of the preset Earth (radius 2) has room for.
pub const EARTH_PEOPLE: f64 = 8.0e9;
/// Worlds where a spacesuit is needed only have room for bases: this share of the above.
const BASES_SHARE: f64 = 0.001;
/// Growth per simulated second while there is plenty of room (doubling every ~14 s).
const GROWTH: f64 = 0.05;
/// How fast people move away from a world that is too crowded for its conditions...
const MOVING_AWAY: f64 = 0.08;
/// ...and from one where nobody can live at all.
const FLEEING: f64 = 0.5;
/// The first ship to an empty world carries this many, and sets out from a world with at least `SETTLER_SOURCE`.
const SETTLERS: f64 = 1000.0;
const SETTLER_SOURCE: f64 = 1.0e6;
/// The population is brought up to date about this often (simulated seconds).
pub const INTERVAL: f64 = 1.0;

/// How many people `b` has room for, given its conditions.
pub fn capacity(c: &Conditions, b: &Body) -> f64 {
    let room = EARTH_PEOPLE * (b.radius / 2.0).powi(2);
    match c.level {
        Level::Good => room,
        Level::Caution => room * BASES_SHARE,
        Level::Danger => 0.0,
    }
}

/// Starting populations for a freshly built universe: every just-right world is full.
pub fn settle(u: &mut Universe) {
    let full: Vec<f64> = u
        .bodies
        .iter()
        .map(|b| {
            let c = conditions::conditions(u, b);
            if c.level == Level::Good { capacity(&c, b) } else { 0.0 }
        })
        .collect();
    for (b, p) in u.bodies.iter_mut().zip(full) {
        b.population = p;
    }
}

/// Move every population on by `dt` simulated seconds.
pub fn update(u: &mut Universe, dt: f64) {
    // Work out the conditions first (they need to look at the whole universe).
    let rooms: Vec<(f64, bool)> = u
        .bodies
        .iter()
        .map(|b| {
            let c = conditions::conditions(u, b);
            (capacity(&c, b), c.level == Level::Good)
        })
        .collect();
    // Where settlers would come from: the most populous world, if it is big enough.
    let home = u.bodies.iter().filter(|b| b.population >= SETTLER_SOURCE).max_by(|a, b| a.population.total_cmp(&b.population)).map(|b| b.name.clone());
    let mut news = Vec::new();
    for (b, (room, just_right)) in u.bodies.iter_mut().zip(rooms) {
        let p = b.population;
        if p <= 0.0 {
            if let (true, Some(home)) = (just_right, &home) {
                b.population = SETTLERS.min(room);
                if !b.name.is_empty() && !home.is_empty() {
                    news.push(format!("🏠 Settlers from {home} have landed on {}", b.name));
                }
            }
            continue;
        }
        b.population = if room <= 0.0 {
            p * (-FLEEING * dt).exp()
        } else if p < room {
            // Fast while there is room, levelling off as the world fills up.
            room / (1.0 + (room / p - 1.0) * (-GROWTH * dt).exp())
        } else {
            room + (p - room) * (-MOVING_AWAY * dt).exp()
        };
        if b.population < 1.0 {
            b.population = 0.0;
            if !b.name.is_empty() {
                news.push(format!("🚀 The last people have left {} for somewhere nicer", b.name));
            }
        }
    }
    for n in news {
        u.event(n);
    }
}

/// "8.0 billion", "2.3 million", "12 thousand", "850", "Nobody".
pub fn fmt_people(n: f64) -> String {
    let n = n.max(0.0);
    let big = |v: f64, word: &str| if v >= 99.95 { format!("{v:.0} {word}") } else { format!("{v:.1} {word}") };
    if n < 1.0 {
        "Nobody".to_string()
    } else if n < 999.5 {
        format!("{n:.0}")
    } else if n < 999_500.0 {
        format!("{:.0} thousand", n / 1e3)
    } else if n < 999.5e6 {
        big(n / 1e6, "million")
    } else if n < 999.5e9 {
        big(n / 1e9, "billion")
    } else {
        big(n / 1e12, "trillion")
    }
}

/// The same, short enough for a slider's value box: "8.0B", "2.3M", "12k", "850".
pub fn fmt_people_short(n: f64) -> String {
    let n = n.max(0.0);
    if n < 999.5 {
        format!("{n:.0}")
    } else if n < 999_500.0 {
        format!("{:.0}k", n / 1e3)
    } else if n < 999.5e6 {
        format!("{:.1}M", n / 1e6)
    } else if n < 999.5e9 {
        format!("{:.1}B", n / 1e9)
    } else {
        format!("{:.1}T", n / 1e12)
    }
}

/// Read back what `fmt_people_short` writes (and plain numbers), for typing into the box.
pub fn parse_people(text: &str) -> Option<f64> {
    let t = text.trim().replace(',', "");
    let (digits, scale) = match t.chars().last()?.to_ascii_lowercase() {
        'k' => (&t[..t.len() - 1], 1e3),
        'm' => (&t[..t.len() - 1], 1e6),
        'b' => (&t[..t.len() - 1], 1e9),
        't' => (&t[..t.len() - 1], 1e12),
        _ => (&t[..], 1.0),
    };
    digits.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0).map(|v| v * scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::BodyKind;
    use crate::presets::{self, Preset};

    fn find<'a>(u: &'a Universe, name: &str) -> &'a Body {
        u.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no {name}"))
    }

    fn run(u: &mut Universe, seconds: f64) {
        for _ in 0..(seconds * 6.0) as usize {
            u.update(1.0 / 60.0, 10.0, false);
        }
    }

    #[test]
    fn solar_system_starts_with_people_on_earth_only() {
        let mut u = Universe::new("t", 1);
        presets::build(&mut u, Preset::SolarSystem);
        assert!((find(&u, "Earth").population - EARTH_PEOPLE).abs() < 1.0, "Earth has {}", find(&u, "Earth").population);
        for b in u.bodies.iter().filter(|b| b.name != "Earth") {
            assert_eq!(b.population, 0.0, "{} should be empty", b.name);
        }
        // Left alone, it stays that way: Earth is full, and nowhere else is fit to settle.
        run(&mut u, 120.0);
        assert!((find(&u, "Earth").population / EARTH_PEOPLE - 1.0).abs() < 0.01);
        assert!(u.bodies.iter().filter(|b| b.population > 0.0).count() == 1);
    }

    #[test]
    fn settlers_fill_a_new_world_that_is_just_right() {
        let mut u = Universe::new("t", 1);
        presets::build(&mut u, Preset::SolarSystem);
        // A twin of Earth, sharing its orbit on the far side of the Sun.
        let (sun, earth) = (find(&u, "Sun").clone(), find(&u, "Earth").clone());
        let mut twin = u.new_body("Twin", BodyKind::Earthlike, sun.pos * 2.0 - earth.pos, sun.vel * 2.0 - earth.vel);
        twin.mass = earth.mass;
        twin.radius = earth.radius;
        twin.surface.atmo_strength = earth.surface.atmo_strength;
        u.add(twin);
        run(&mut u, 5.0);
        let first = find(&u, "Twin").population;
        assert!(first >= SETTLERS && first < 1.0e6, "the first settlers should have landed, found {first}");
        assert!(u.log.iter().any(|l| l.contains("Settlers from Earth have landed on Twin")), "{:?}", u.log);
        run(&mut u, 600.0);
        assert!(find(&u, "Twin").population > EARTH_PEOPLE * 0.9, "the twin should have filled up, it has {}", find(&u, "Twin").population);
    }

    #[test]
    fn nobody_settles_an_empty_universe() {
        // No populated world to set out from: a just-right world stays empty.
        let mut u = Universe::new("t", 1);
        presets::build(&mut u, Preset::TheiaImpact);
        assert!(u.bodies.iter().all(|b| b.population == 0.0), "the young Earth has no people yet");
        run(&mut u, 3.0);
        assert!(u.bodies.iter().all(|b| b.population == 0.0));
    }

    #[test]
    fn people_move_away_when_a_world_turns_deadly() {
        let mut u = Universe::new("t", 1);
        presets::build(&mut u, Preset::SolarSystem);
        let i = u.bodies.iter().position(|b| b.name == "Earth").unwrap();
        // Lose the air: a spacesuit world only has room for bases.
        u.bodies[i].surface.atmo_strength = 0.0;
        run(&mut u, 200.0);
        let p = find(&u, "Earth").population;
        assert!(p > 1.0e6 && p < EARTH_PEOPLE * 0.01, "only bases should remain, found {p}");
        // Turn it into a lava world: nobody can stay.
        u.bodies[i].kind = BodyKind::Lava;
        run(&mut u, 60.0);
        assert_eq!(find(&u, "Earth").population, 0.0);
        assert!(u.log.iter().any(|l| l.contains("The last people have left Earth")), "{:?}", u.log);
    }

    #[test]
    fn people_escape_when_their_world_is_destroyed() {
        let mut u = Universe::new("t", 1);
        presets::build(&mut u, Preset::HeadOn);
        let aqua = find(&u, "Aqua").id;
        u.get_mut(aqua).unwrap().population = 2.0e6;
        run(&mut u, 30.0);
        assert!(u.bodies.iter().all(|b| b.name != "Aqua"), "Aqua should have been smashed");
        assert!(u.log.iter().any(|l| l.contains("million people of Aqua escaped in their rockets")), "{:?}", u.log);
        assert!(!u.log.iter().any(|l| l.contains("died") || l.contains("killed")));
    }

    #[test]
    fn people_are_written_in_words() {
        assert_eq!(fmt_people(0.0), "Nobody");
        assert_eq!(fmt_people(850.0), "850");
        assert_eq!(fmt_people(12_300.0), "12 thousand");
        assert_eq!(fmt_people(2.3e6), "2.3 million");
        assert_eq!(fmt_people(8.0e9), "8.0 billion");
        assert_eq!(fmt_people(250.0e9), "250 billion");
        for n in [0.0, 850.0, 12_000.0, 2.3e6, 8.0e9, 1.5e12] {
            assert_eq!(parse_people(&fmt_people_short(n)), Some(n), "{}", fmt_people_short(n));
        }
        assert_eq!(parse_people("1,000"), Some(1000.0));
        assert_eq!(parse_people("lots"), None);
        assert_eq!(parse_people("-5"), None);
    }
}
