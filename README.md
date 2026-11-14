# 🌍 Arthur's Universe Playground

A space sandbox written in **Rust + Vulkan**. Start with the Solar System, then:

- **Terraform planets.** Paint continents, dig oceans, and add lava, ice, forests, sand or craters directly onto planet surfaces.
- **Spawn new worlds.** Click to drop a planet into a circular orbit, or click-and-drag to fling it, with a live predicted trajectory (it turns red if it's going to crash).
- **Smash planets together.** Collisions use an impact-energy model: gentle hits merge (leaving craters and molten scars), harder ones shatter worlds into glowing fragments and debris that can re-accrete.
- **Create your own universe.** Pick a centre (Sun-like star, red dwarf, blue giant, binary, black hole, nothing), the number of planets, moons, belts, rings, gravity strength and chaos.
- **Cosmic phenomena:**
  - **Black holes:** accretion disks with Doppler beaming, gravitational lensing, and tidal disruption ("spaghettification").
  - **Supernovae:** a blast wave that pushes and scorches planets, an expanding nebula, and a neutron-star or black-hole remnant.
  - **Stellar mergers, black-hole mergers and galaxy collisions** with tidal tails.

## Download

Grab the latest build from [Releases](https://github.com/jonbp/arthurs-universe-playground/releases/latest):

| Platform | File | Notes |
|---|---|---|
| Windows 10/11 (x64) | `…-windows-x64.zip` | Needs a Vulkan-capable GPU driver (standard with NVIDIA, AMD and Intel drivers). |
| macOS 11+ (Apple Silicon) | `…-macos-arm64.dmg` | MoltenVK is bundled. The app is not notarized: on first launch, right-click the app and choose **Open**. |
| Linux (x86_64) | `…-linux-x86_64.tar.gz` | Needs `libvulkan1` and a Vulkan driver (Mesa or vendor). Run `./install.sh` to add it to your app menu. |

See [CHANGELOG.md](CHANGELOG.md) for what changed in each version.

## Presets

| Preset | What happens |
|---|---|
| The Solar System | Sun, 8 planets plus Pluto, the Moon, Galilean moons, Titan, Triton, rings, asteroid belt |
| Solar System vs Rogue Black Hole | A black hole plows through the Solar System |
| Theia Impact | A Mars-sized world hits the proto-Earth |
| Head-on Planet Smash | Two worlds collide head-on |
| Binary Star System | Circumbinary planets around two suns |
| Black Hole Feast | A supermassive black hole, a doomed star and planets |
| Supernova Countdown | A blue supergiant explodes after 20 seconds |
| Galaxy Collision | Two ~34k-star galaxies collide |
| Protoplanetary Disk | Planetesimals merge into planets |
| Empty Universe | Build from scratch |

## Controls

| Input | Action |
|---|---|
| Right-drag (or left-drag in Select) | Orbit camera |
| Scroll / pinch | Zoom |
| Middle-drag / Shift+drag | Pan |
| Double-click / `F` | Follow selected body |
| `1`–`4` | Select / Spawn / Terraform / Smash tools |
| `Space` | Pause |
| `[` `]` or `-` `+` | Slow down / speed up time |
| `B` | Asteroid barrage at selection |
| `Del` | Delete selection |
| `T` / `L` / `H` | Trails / labels / hide UI |

## Building

Requirements: Rust (1.85+) and MoltenVK on macOS.

```sh
brew install rust molten-vk vulkan-loader
cargo run --release
```

To build a self-contained macOS app (MoltenVK embedded, no Vulkan SDK needed to run it):

```sh
./scripts/bundle_macos.sh
open "target/release/bundle/Arthur's Universe Playground.app"
```

The code is portable Vulkan, so it also builds on Linux and Windows with a Vulkan driver installed.

## Releasing

CI (`.github/workflows/ci.yml`) builds and tests on Windows, macOS and Linux for every push and pull request. To cut a release:

1. Bump `version` in `Cargo.toml` and run `cargo build` to update `Cargo.lock`.
2. Move the `[Unreleased]` notes in `CHANGELOG.md` under a new `## [x.y.z] - YYYY-MM-DD` heading and update the compare links.
3. Commit, then tag and push: `git tag vx.y.z && git push origin main vx.y.z`.

`.github/workflows/release.yml` then builds the Windows `.zip` (icon embedded in the `.exe`), the macOS `.dmg` (`.app` with `AppIcon.icns`) and the Linux `.tar.gz` (`.desktop` entry plus icon). It publishes them to a GitHub release, using that version's `CHANGELOG.md` section as the release notes. All icons are rendered from `assets/AppIcon.svg` by `build.rs`.

## How it works

- **Physics** (`src/universe.rs`): f64 N-body gravity with a kick-drift-kick leapfrog integrator and adaptive sub-stepping.
  - Collisions use the specific impact energy Q against a catastrophic-disruption threshold Q*_RD (after Leinhardt & Stewart 2012) to decide between merging, cratering and fragmentation.
  - Black holes disrupt bodies inside the tidal radius r_t = R·(2M/m)^⅓.
  - Supernovae send out a shock front that applies impulses and scorches surfaces.
  - Particles (debris, nebulae, galaxy stars) are test masses pulled by the most massive bodies.
- **Rendering** (`src/renderer.rs`, `shaders/`): raw Vulkan through `ash`.
  - Shaders are written in WGSL and compiled to SPIR-V at build time by `naga` (see `build.rs`), so no shader SDK is needed.
  - A reverse-Z HDR scene pass draws instanced procedural planets with vertex displacement, bump-mapped terrain, oceans, clouds and atmospheres, plus the paint texture array. Rings, accretion disks, additive particles and trails follow.
  - A blit-generated mip chain provides bloom. The composite pass applies point-lens gravitational lensing and ACES tonemapping. The egui UI is drawn on top.
  - Rendering is camera-relative, so f32 precision holds even across galaxy-sized scenes.
- **Tests**: `cargo test --release` covers orbit stability, momentum conservation, shattering, supernova remnants, spaghettification, trajectory prediction, the paint brushes, every preset, and the tools driven through the input layer.

The UI font is [Inter](https://rsms.me/inter/) (SIL Open Font License, see `assets/fonts/Inter-LICENSE.txt`). It is compiled into the binary. UI icons are a custom line-icon set drawn as vectors (`src/icons.rs`): 24-unit grid, round caps, one stroke weight, so they stay sharp at any size and never depend on font glyphs.

Developer flags for automated screenshots: `--preset <name> --focus <body> --zoom <d> --speed <x> --screenshot out.bmp --frames N --hide-ui`. `--icons` (or `--glyph-test`) shows the whole icon set at large and small sizes (see `--help`).
