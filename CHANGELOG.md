# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.3.1] - 2026-11-14

### Added

- **Quit button** at the right end of the top-right bar. Your universe is saved automatically on the way out, as when you close the window.

### Changed

- The event log and the flying and walking panels (the Sparrow, "Standing on…") are frosted glass like the side panels: darkened, with the universe blurred behind them, so they stay easy to read over bright skies.
- Fresh README screenshots with these changes.

## [1.3.0] - 2026-11-14

### Added

- **Save your universes.** In the universe menu, **Saved universes…** lets you save the current universe under a name and load or delete saves. A save keeps every body and its orbit, painted terrain, rings, the stars of a galaxy, physics settings, the camera and the time speed. Saving a name that already exists replaces it, and deleting needs a second click to confirm.
- **Continue where you left off.** The game saves automatically when you close it (and every 3 minutes while you play). **Continue where you left off** in the universe menu brings that back.
- **Sound.** Impacts, smashes, star and black-hole mergers, supernovae, rocket lift-off, landing and crashes, the engine roar following the throttle, footsteps, new planets popping in, projectiles whooshing and soft button clicks. Effects get quieter with distance and are panned to where they happen. Everything is synthesized live; there are no audio files.
- **Music.** A calm ambient soundtrack that drifts between four chords with bell notes on top and never repeats exactly.
- **Sound settings.** Mute (`M`, or the new speaker button in the top-right bar), plus overall, effects and music volume in Settings. Easy flying can now also be switched on in Settings under Gameplay.

### Changed

- A more immersive look: the top bars, tool dock, event log, hint bar and flight and walking panels no longer sit in solid boxes. Each floats on a light haze that fades into space around it, and buttons, tiles and stat cells only light up when you hover or pick them. The windows that open beside the scene (tool cards, inspector, Settings) are frosted glass: the universe behind them is blurred and darkened, with no border. Menus and dialogs stay solid.
- Settings are split into tabs with icons: Display, Graphics, Sound, Interface and Gameplay, so the panel no longer scrolls on and on.
- Linux builds need the ALSA development headers (`libasound2-dev`), and the game uses `libasound2` at run time for sound.

## [1.2.2] - 2026-11-14

### Added

- **Easy flying.** A new switch in the Hangar (and a button on the flight panel, or press `F` while flying) for younger pilots and relaxed trips: the fuel tank never runs dry, every rocket has enough power to lift off from any world, touching down on solid ground always lands safely however fast you come in, and the rocket keeps itself level. `A`/`D` turn left and right like an aeroplane. Stars, black holes and gas giants are still dangerous. The choice is saved with your settings.

### Fixed

- Clicking the Sun no longer sends every planet flying out of the Solar System. The inspector's Mass slider stopped at 200,000 and quietly cut the Sun's mass (400,000) in half as soon as it was shown. Now showing the inspector never changes a body, and the Mass slider goes up to 1,000,000.
- Selected items are readable again: the chosen entry in the speed menu (such as `1×`) and text you highlight in a name box were drawn in yellow on yellow. They now use dark text, and text boxes keep their yellow outline while you type.

## [1.2.1] - 2026-11-14

### Added

- **Settings.** A new Settings panel (the gear in the top-right bar) with:
  - **Display:** fullscreen, VSync on or off, and a frame-rate limit (30, 60, 120, 144 or 240 FPS).
  - **Resolution scale** from 50% to 200%: draw the 3D scene at a lower resolution for speed on modest graphics cards, or above 100% for smoother edges. The interface stays sharp at any scale.
  - **Graphics:** planet detail (Low, Standard, High), exposure, glow, stars, gravitational lensing and orbit trails. These replace the old Visuals pop-up.
  - **Interface:** UI size (75% to 150%), labels, and the bodies / particles / FPS counter.
  - Settings are saved automatically and restored the next time you start the game.
- **Fullscreen mode**: press `F11`, or switch it on in Settings.

### Fixed

- Windows: the game no longer opens a command-prompt window alongside it. When started from a terminal, its output still appears in that terminal, and a startup error is shown in a message box instead of disappearing.

## [1.2.0] - 2026-11-14

### Added

- **Fly a rocket.** A new Fly tool (`5`) with a hangar of six rockets: Sparrow, Moonhopper, Goliath, Star Shuttle, Ion Glider and Warp Arrow, each with its own engine power, fuel, steering and landing legs. Launch from any solid world, or start in orbit around a star or gas giant. The rocket feels the gravity of every body, shows its predicted path, lands if you touch down gently and is wrecked if you don't. Switch between an outside view and a cockpit with an attitude ball (`V`).
- **Walk on worlds.** A new Walk tool (`6`): click a rocky planet, moon or asteroid to stand on that spot, walk over its real terrain (including anything you have painted), and jump, higher where gravity is weaker. Land a rocket, press `G` to climb out, and `G` again to climb back in. Inside an atmosphere the daytime sky hides the stars and turns orange at sunset.
- **Conditions on every world.** The inspector shows temperature (with the day/night swing on airless worlds), gravity, air pressure and what the air is made of, sunlight, day length, escape speed, the state of water, and a verdict on whether people could live there. On foot you get the temperature where you are standing.
- **Update check.** On startup the game asks GitHub whether a newer release exists and, if so, shows a banner with a Download button for your platform. The help window shows the current version and a "Check for updates" button. Start with `--no-update-check` (or set `AUP_NO_UPDATE_CHECK`) to switch it off.

### Changed

- Planet terrain uses an integer-hashed noise that gives identical results on the GPU and the CPU, so rockets and astronauts stand exactly on the ground that is drawn. Continents, craters and cloud patterns have the same character but different shapes than in 1.1.0.
- Worlds that fill the view are drawn with a finer sphere and close-up ground detail.
- Faster planet rendering: the hidden far side of each sphere is no longer shaded, the terrain noise is evaluated once per pixel instead of twice, and the starfield is only computed where sky is actually visible.

### Fixed

- Debug builds (`cargo run` without `--release`) no longer abort on the first frame with "Dropped TexturesDelta".

## [1.1.0] - 2026-11-14

### Fixed

- Clicking a panel button no longer also clicks "through" it into the scene, which could deselect the current body or fire a projectile at whatever was behind the panel.
- A star collapsed into a black hole (or retyped in the inspector) while on a supernova countdown no longer explodes anyway.
- A neutron star that swallows a star stays compact instead of ballooning to star size.
- Particles falling into a body are counted once, not once per sub-step, so black-hole accretion disks no longer over-brighten at high time warp.
- Holding Space, B or other shortcut keys no longer repeats the action many times a second.
- Mouse buttons no longer stay "held" when the window loses focus mid-drag.
- UI texture updates are no longer dropped when they coincide with a window resize.
- Turning a painted planet into a star or black hole in the inspector releases its paint layer.
- The game no longer spins a CPU core while minimised.

### Changed

- Faster simulation: the gravity loop works on a compact copy of the bodies, and large particle counts are integrated on several threads (about 20% faster for the protoplanetary disk and 45% for the galaxy collision at high time warp).
- Frame buffers are reused between frames, and loading a universe only resets paint layers that were actually painted.

## [1.0.0] - 2026-11-14

First public release.

### Added

- Vulkan-rendered space sandbox, starting from the Solar System.
- Terraforming brushes: continents, oceans, lava, ice, forests, sand and craters.
- Planet spawning into circular orbits or flung by click-and-drag, with a live predicted trajectory.
- Impact-energy collision model: merging, cratering and fragmentation with re-accreting debris.
- Universe builder: choose the centre (star, red dwarf, blue giant, binary, black hole or nothing), planets, moons, belts, rings, gravity and chaos.
- Cosmic phenomena: black holes with accretion disks, gravitational lensing and spaghettification; supernovae with blast waves and remnants; stellar, black-hole and galaxy mergers.
- Ten presets, including Theia Impact, Black Hole Feast, Supernova Countdown and Galaxy Collision.
- Downloadable builds for Windows (x64), macOS (Apple Silicon, MoltenVK bundled) and Linux (x86_64), all with the app icon.

[Unreleased]: https://github.com/jonbp/arthurs-universe-playground/compare/v1.3.1...HEAD
[1.3.1]: https://github.com/jonbp/arthurs-universe-playground/compare/v1.3.0...v1.3.1
[1.3.0]: https://github.com/jonbp/arthurs-universe-playground/compare/v1.2.2...v1.3.0
[1.2.2]: https://github.com/jonbp/arthurs-universe-playground/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/jonbp/arthurs-universe-playground/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/jonbp/arthurs-universe-playground/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/jonbp/arthurs-universe-playground/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/jonbp/arthurs-universe-playground/releases/tag/v1.0.0
