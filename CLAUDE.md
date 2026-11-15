# CLAUDE.md

Notes for working on Arthur's Universe Playground: a space sandbox in Rust (edition 2024) with a hand-written Vulkan renderer (`ash`) and an `egui` interface. It is played by a child, Arthur: keep wording plain and friendly, controls forgiving, and nothing frightening or punishing by default (see Easy flying).

The project is fully vibe coded (the README says so). Every change is made by an AI assistant from plain-language requests, so keep the code, tests and docs in a state a fresh session can pick up from.

## Commands

```sh
cargo build --release          # always test and run release builds; debug is slow
cargo test --release           # ~100 tests, ~25 s; CI runs `--locked`
./target/release/arthurs-universe-playground
```

- CI (`.github/workflows/ci.yml`) only builds and tests on Linux, macOS and Windows. It does **not** run `cargo fmt` or clippy.
- The repo is **not** rustfmt-clean and has some existing clippy warnings. Don't run `cargo fmt` over the repo (huge unrelated diff); match the surrounding style by hand, and don't add new warnings.
- Linux needs `libasound2-dev` (sound, via cpal) and a Vulkan loader. macOS needs MoltenVK (`brew install molten-vk vulkan-loader`).

## Seeing a change in the real app

Prefer checking UI and rendering changes in the actual app over guessing. The binary has developer flags for automated screenshots (full list: `--help`):

```sh
B=./target/release/arthurs-universe-playground
$B --no-update-check --preset solar --focus Earth --zoom 9 --frames 120 --screenshot out.bmp
$B --no-update-check --fly Earth --rocket 0 --hold z --frames 75 --screenshot fly.bmp
$B --no-update-check --walk Mars --frames 90 --screenshot walk.bmp
$B --no-update-check --preset galaxy --speed 30 --zoom 1000 --frames 220 --hide-ui --screenshot g.bmp
```

- `--click X,Y@FRAME` injects a UI click. Coordinates are egui **points** (window pixels / pixels-per-point, i.e. half the pixel size on Retina). Repeat the flag for several clicks.
- `--saves` opens the Saves window; `--icons` shows the whole icon set; `--cockpit`, `--rocket <0-5>`, `--hold <key,key>` set up flights.
- Screenshot runs don't read or write the user's settings or saves, and don't open a sound device. To test with particular settings, point `AUP_SETTINGS` at a scratch file (e.g. `{"labels": false}`); saves then go in a `saves` folder next to it. Never run tests against the real settings or saves.
- Synthetic clicks are unreliable while another copy of the game is open (they fight over focus). Close the user's game first or expect flaky results.
- `--hide-ui` still draws body labels and a "Press H" hint at the top; crop the top or turn labels off via `AUP_SETTINGS`.
- `--zoom` is a distance in game units (1 AU = 46,962) and `--speed` a time scale (1 = 30 minutes a second, 48 = a day a second). After `--focus` allow about 120 frames for the camera to arrive.
- Sim time per frame is not constant (adaptive sub-stepping); the log line `sim t=` tells you where a screenshot landed.
- `sips` and ImageMagick (`magick`) are available for converting and composing images. There's no PIL.

## Layout

| File | What's in it |
|---|---|
| `src/main.rs` | winit app, CLI flags, frame loop, settings persistence, autosave on `exiting`, quit, sound bridge, UI blur rects |
| `src/game.rs` | `Game`: input, tools, selection, spawning, smashing. Owns `Universe`, `Camera`, `Settings`, `Saves` |
| `src/universe.rs` | N-body physics, collisions, particles, flashes, supernovae, `Sound` events |
| `src/explore.rs` | Flying rockets (incl. easy flying) and walking on worlds |
| `src/save.rs` | Save file format and the Saves window state |
| `src/population.rs` | How many people live on each world: growth, moving away, settlers; `Universe::evacuate` when a world is destroyed |
| `src/audio.rs` | Synthesized sound: `Mixer` (testable), `Audio` (cpal device), `SoundBridge` (game -> mixer) |
| `src/settings.rs` | `Settings` with JSON load/save; `sanitize` clamps values |
| `src/ui.rs`, `src/ui/hud.rs`, `src/ui/options.rs` | egui interface: bars, dock, tool cards, inspector, modals; flight/walk HUDs; Settings tabs |
| `src/icons.rs` | Hand-drawn vector icon set on a 24x24 grid |
| `src/renderer.rs`, `src/scene.rs`, `src/gpu_types.rs`, `shaders/*.wgsl` | Vulkan renderer. `build.rs` compiles WGSL to SPIR-V with naga and validates it at build time |
| `src/presets.rs` | The preset universes (the Solar System's tables of real figures) and "Create your own universe" |
| `src/units.rs` | Real-world units (km, seconds, masses from GM) and how distances, speeds, times and masses are worded |

## Conventions and traps

**UI styling** (`src/ui.rs`):
- `glass()`: the light haze behind the top bars, tool dock and hint bar.
- `window()` + `window_panel()` / `frosted_panel()`: frosted glass (50% black, no border) for the tool card, inspector, Settings, event log and flight/walk HUDs. These record their rect so the composite shader blurs the scene behind them; use them for any new panel of that kind. At most `MAX_UI_BLUR` (6) frosted panels show at once.
- `solid()`: menus and modal dialogs.
- Accent yellow is `ACCENT`; text on it must be `ON_ACCENT`. egui draws selected text in `visuals.selection.stroke`, which is deliberately dark.
- Use `slider_row()` for sliders: it sets `SliderClamping::Edits`. egui's default clamping silently rewrites out-of-range values every frame (that once halved the Sun's mass when it was selected).
- Use `text_field()` for framed text boxes (it draws the focus ring).

**GPU uniforms**: `Globals` in `src/gpu_types.rs` must match `shaders/globals.wgsl` field for field (vec4-sized members, array lengths included). Change both together.

**Settings**: new fields need a default, `to_json`, `from_json` (missing keys keep defaults so old files load), `sanitize` if numeric, and the round-trip test in `settings.rs`. Put the control in the right Settings tab in `src/ui/options.rs`.

**Saves**: `save.rs` writes a JSON header plus binary paint layers and particles. New fields must be optional on load with sensible defaults. Bump `VERSION` only for incompatible changes (newer versions are refused). `serde_json` uses `float_roundtrip` so positions reload exactly; keep it.

**Sound**: to add an effect, add a `Sound` variant (`universe.rs`), push it with `universe.sound(...)` where it happens, map it in `sfx_for` and give it a `recipe` in `audio.rs`. Listen without playing: `AUP_DEMO_WAV=demo.wav cargo test --release demo_wav -- --ignored`. Keep impacts deep rather than hissy, and music bells sparse and soft (the user found them annoying when frequent).

**Scale** (`src/units.rs`): universes are built to scale (`Universe::real_scale`) except the two models (Galaxy Collision, Protoplanetary Disk) and saves from before 1.5.
- G = 1, one unit of length is half the Earth's radius (3185.5 km), one unit of time is 1800 s; masses come from real GM values via `units::mass`. So Earth has radius 2 and mass 39.95, the Sun radius 218 and mass 1.33e7, 1 AU = 46,962 units, a day = 48 and a year = 17,532 units of time. Build new presets with `units::au`, `km`, `mass`, `spin_rate` rather than bare numbers.
- Anything scale-dependent takes the flag: `BodyKind::defaults(real)`, `black_hole_radius(mass, real)`, `Body::luminosity(real)`, the yardstick in `conditions.rs`, and the `units::fmt_*` functions (plain numbers in a model). `Universe::new` starts as a model (`real_scale = false`), which is what most physics tests use.
- Drawn bigger than life on purpose: neutron stars and small black holes (`MIN_BLACK_HOLE_RADIUS`), rockets and astronauts (`explore::person_scale` shrinks the astronaut on tiny moons).
- Planets are sub-pixel from afar. `scene.rs` draws far-off worlds as dots and keeps stars a fixed size on screen, `game.rs::orbit_lines` draws each named body's orbit (toggled with trails; a body with an orbit line gets no trail), and the particle shader never dims a sub-pixel particle below a tenth.
- Particle positions are f32: fine to about Saturn's distance, grainy (0.1 unit) by Neptune's. Bodies and the rocket are f64.
- The supernova scales its blast by the star's radius (`k` in `explode_star`). The Theia and Head-on scenes keep their hand-tuned (slightly large) worlds, lit by a real-size but nearly weightless `lamp_sun`.

**Physics** (`src/universe.rs`):
- The step length adapts (`longest_step`): at most `STEP_ORBIT_FRACTION` of the tightest orbit's timescale and `STEP_APPROACH_FRACTION` of the time for any two bodies to meet, rounded down to a power of two, never below the old fixed 0.05, and 0.05 while a rocket is flying. Up to `FINE_STEPS` steps a frame are taken at `base_dt` first; the Theia preset's outcome is sensitive to that number.
- "What does this orbit?" is `dominant_attractor`: strongest tidal grip (mass / distance³), not strongest pull, or the Moon would be said to orbit the Sun.
- Particles can carry `mass` (rubble from a smash, a shredded world's stream; zero for decoration and galaxy stars). A body that swallows one gains its mass and momentum, so when a collision turns mass into dust, hand it over with `give_mass_to_newest` rather than dropping it. Collision tests check that mass and momentum add up, dust included.
- A glancing hit goes through `graze` first: it works out which part of the smaller world is in the way (that part merges or shatters as usual) and which part sails past as a slowed "remnant" shedding pieces. Nothing is placed in orbit by hand; `name_new_moons` only labels a leftover that gravity has left circling. Whether a moon results depends on the hit and on chance (about half the time in the best case), so the Theia preset fixes its seed. When judging anything random like this, sweep many seeds, not one.
- Work per frame is capped: `pair_budget` for bodies (tuned to the computer by `Game::tune_physics`, only when `physics_budget` is set, so tests and screenshot runs stay repeatable) and `PARTICLE_WORK` for particles. Check the cost of physics changes with many bodies at 100x, not just the presets at 1x.

**People** (`src/population.rs`): `Body::population` follows the world's `conditions` verdict (Good: full room, Caution: bases only, Danger: none). Nobody is ever hurt: anything that destroys or tears a world must call `Universe::evacuate` first, and wording about people stays gentle (they "escape" or "leave", never die). New presets get their starting people from `population::settle`.

**Tests**: `Game` runs headless (no GPU, no audio), so gameplay is tested by driving `Game` directly; see the `tests` modules in `game.rs`, `explore.rs`, `save.rs`, `audio.rs`. UI tests need `setup_style(&ctx)` first (fonts). Add a regression test with each bug fix and check it fails without the fix.

## Releasing

The user rolls features into releases (1.2.2, 1.3.0, 1.3.1...). When asked to release:

1. Group the work into **at most three commits** (the user asked for this on 2026-11-14, replacing the earlier one-commit-per-feature rule), each building and passing tests on its own: for example gameplay, graphics, then the release commit.
2. Final commit: bump `version` in `Cargo.toml`, `cargo build` to update `Cargo.lock`, move the `[Unreleased]` notes in `CHANGELOG.md` under `## [x.y.z] - YYYY-MM-DD` and update the compare links. Message: `Arthur's Universe Playground x.y.z` with short `Added:` / `Changed:` / `Fixed:` lists.
3. `git tag -a vx.y.z -m "Arthur's Universe Playground x.y.z"` and `git push origin main vx.y.z`. The tag triggers `release.yml`, which tests, packages the Windows .zip, macOS .dmg and Linux .tar.gz, and publishes the GitHub release with that CHANGELOG section as notes. Watch it with `gh run watch`.

Changelog entries are written for players, in plain language (Keep a Changelog format). Commit messages end with the `Co-Authored-By` line from the session's attribution instructions.

README screenshots live in `docs/screenshots/` as 1600-px-wide JPEGs (quality ~82). Recapture the ones showing the interface when the UI changes.
