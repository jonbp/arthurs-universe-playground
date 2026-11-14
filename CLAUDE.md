# CLAUDE.md

Notes for working on Arthur's Universe Playground: a space sandbox in Rust (edition 2024) with a hand-written Vulkan renderer (`ash`) and an `egui` interface. It is played by a child, Arthur: keep wording plain and friendly, controls forgiving, and nothing frightening or punishing by default (see Easy flying).

The project is fully vibe coded (the README says so). Every change is made by an AI assistant from plain-language requests, so keep the code, tests and docs in a state a fresh session can pick up from.

## Commands

```sh
cargo build --release          # always test and run release builds; debug is slow
cargo test --release           # ~78 tests, ~30 s; CI runs `--locked`
./target/release/arthurs-universe-playground
```

- CI (`.github/workflows/ci.yml`) only builds and tests on Linux, macOS and Windows. It does **not** run `cargo fmt` or clippy.
- The repo is **not** rustfmt-clean and has some existing clippy warnings. Don't run `cargo fmt` over the repo (huge unrelated diff); match the surrounding style by hand, and don't add new warnings.
- Linux needs `libasound2-dev` (sound, via cpal) and a Vulkan loader. macOS needs MoltenVK (`brew install molten-vk vulkan-loader`).

## Seeing a change in the real app

Prefer checking UI and rendering changes in the actual app over guessing. The binary has developer flags for automated screenshots (full list: `--help`):

```sh
B=./target/release/arthurs-universe-playground
$B --no-update-check --preset solar --focus Earth --zoom 9 --frames 90 --screenshot out.bmp
$B --no-update-check --fly Earth --rocket 0 --hold z --frames 75 --screenshot fly.bmp
$B --no-update-check --walk Mars --frames 90 --screenshot walk.bmp
$B --no-update-check --preset galaxy --speed 30 --zoom 1000 --frames 220 --hide-ui --screenshot g.bmp
```

- `--click X,Y@FRAME` injects a UI click. Coordinates are egui **points** (window pixels / pixels-per-point, i.e. half the pixel size on Retina). Repeat the flag for several clicks.
- `--saves` opens the Saves window; `--icons` shows the whole icon set; `--cockpit`, `--rocket <0-5>`, `--hold <key,key>` set up flights.
- Screenshot runs don't read or write the user's settings or saves, and don't open a sound device. To test with particular settings, point `AUP_SETTINGS` at a scratch file (e.g. `{"labels": false}`); saves then go in a `saves` folder next to it. Never run tests against the real settings or saves.
- Synthetic clicks are unreliable while another copy of the game is open (they fight over focus). Close the user's game first or expect flaky results.
- `--hide-ui` still draws body labels and a "Press H" hint at the top; crop the top or turn labels off via `AUP_SETTINGS`.
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
| `src/audio.rs` | Synthesized sound: `Mixer` (testable), `Audio` (cpal device), `SoundBridge` (game -> mixer) |
| `src/settings.rs` | `Settings` with JSON load/save; `sanitize` clamps values |
| `src/ui.rs`, `src/ui/hud.rs`, `src/ui/options.rs` | egui interface: bars, dock, tool cards, inspector, modals; flight/walk HUDs; Settings tabs |
| `src/icons.rs` | Hand-drawn vector icon set on a 24x24 grid |
| `src/renderer.rs`, `src/scene.rs`, `src/gpu_types.rs`, `shaders/*.wgsl` | Vulkan renderer. `build.rs` compiles WGSL to SPIR-V with naga and validates it at build time |
| `src/presets.rs` | The preset universes and "Create your own universe" |

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

**Tests**: `Game` runs headless (no GPU, no audio), so gameplay is tested by driving `Game` directly; see the `tests` modules in `game.rs`, `explore.rs`, `save.rs`, `audio.rs`. UI tests need `setup_style(&ctx)` first (fonts). Add a regression test with each bug fix and check it fails without the fix.

## Releasing

The user rolls features into releases (1.2.2, 1.3.0, 1.3.1...). When asked to release:

1. Split the work into **one commit per feature** for easy tracking (the user asked for this), each building and passing tests on its own. When files mix features, build each commit's version of them rather than committing everything at once.
2. Final commit: bump `version` in `Cargo.toml`, `cargo build` to update `Cargo.lock`, move the `[Unreleased]` notes in `CHANGELOG.md` under `## [x.y.z] - YYYY-MM-DD` and update the compare links. Message: `Arthur's Universe Playground x.y.z` with short `Added:` / `Changed:` / `Fixed:` lists.
3. `git tag -a vx.y.z -m "Arthur's Universe Playground x.y.z"` and `git push origin main vx.y.z`. The tag triggers `release.yml`, which tests, packages the Windows .zip, macOS .dmg and Linux .tar.gz, and publishes the GitHub release with that CHANGELOG section as notes. Watch it with `gh run watch`.

Changelog entries are written for players, in plain language (Keep a Changelog format). Commit messages end with the `Co-Authored-By` line from the session's attribution instructions.

README screenshots live in `docs/screenshots/` as 1600-px-wide JPEGs (quality ~82). Recapture the ones showing the interface when the UI changes.
