# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/jonbp/arthurs-universe-playground/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/jonbp/arthurs-universe-playground/releases/tag/v1.0.0
