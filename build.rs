//! Compiles the WGSL shaders in `shaders/` to SPIR-V at build time using naga,
//! so no external Vulkan SDK tooling (glslc etc.) is required.
//!
//! naga has no `#include`, so we concatenate: every shader gets `globals.wgsl`
//! prepended, and scene shaders additionally get `common.wgsl` (bindings + noise).
//!
//! It also rasterises the app logo from `assets/AppIcon.svg` with resvg (pure Rust,
//! works on every platform) into raw RGBA for the UI, so no image decoder or
//! platform tooling is needed at runtime or build time.

use std::{env, fs, path::PathBuf};

fn compile(name: &str, source: &str, out_dir: &PathBuf) {
    let module = match naga::front::wgsl::parse_str(source) {
        Ok(m) => m,
        Err(e) => {
            panic!("WGSL parse error in {name}:\n{}", e.emit_to_string(source));
        }
    };
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let info = match validator.validate(&module) {
        Ok(i) => i,
        Err(e) => panic!("WGSL validation error in {name}:\n{}", e.emit_to_string(source)),
    };
    let mut options = naga::back::spv::Options::default();
    options.lang_version = (1, 3);
    options.flags.remove(naga::back::spv::WriterFlags::ADJUST_COORDINATE_SPACE);
    let words = naga::back::spv::write_vec(&module, &info, &options, None)
        .unwrap_or_else(|e| panic!("SPIR-V emit error in {name}: {e:?}"));
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    fs::write(out_dir.join(format!("{name}.spv")), bytes).unwrap();
}

/// Side length of the rasterised UI logo, in pixels.
const LOGO_PX: u32 = 192;

/// Render the rounded tile of `AppIcon.svg` (x/y 100..924 of its 1024 viewBox) to
/// premultiplied RGBA at `LOGO_PX`×`LOGO_PX`.
fn rasterise_logo(out_dir: &PathBuf) {
    use resvg::{tiny_skia, usvg};
    println!("cargo:rerun-if-changed=assets/AppIcon.svg");
    let data = fs::read("assets/AppIcon.svg").unwrap();
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).expect("AppIcon.svg failed to parse");
    let mut pixmap = tiny_skia::Pixmap::new(LOGO_PX, LOGO_PX).unwrap();
    let s = LOGO_PX as f32 / 824.0;
    resvg::render(&tree, tiny_skia::Transform::from_row(s, 0.0, 0.0, s, -100.0 * s, -100.0 * s), &mut pixmap.as_mut());
    fs::write(out_dir.join("logo.rgba"), pixmap.data()).unwrap();
}

/// Render the full `AppIcon.svg` (rounded tile on a transparent 1024 canvas) to a
/// straight-alpha PNG of side `px`.
fn render_icon_png(tree: &resvg::usvg::Tree, px: u32) -> Vec<u8> {
    use resvg::tiny_skia;
    let mut pixmap = tiny_skia::Pixmap::new(px, px).unwrap();
    // Crop slightly into the 1024 canvas so the tile fills more of small icons.
    let s = px as f32 / 904.0;
    resvg::render(tree, tiny_skia::Transform::from_row(s, 0.0, 0.0, s, -60.0 * s, -60.0 * s), &mut pixmap.as_mut());
    pixmap.encode_png().unwrap()
}

/// Produce platform icon files in OUT_DIR:
/// - `AppIcon-{256,512}.png` (Linux desktop icon, packaged by CI)
/// - `AppIcon.ico` (PNG-compressed multi-size .ico, embedded in the Windows .exe)
fn platform_icons(out_dir: &PathBuf) {
    use resvg::usvg;
    let data = fs::read("assets/AppIcon.svg").unwrap();
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).expect("AppIcon.svg failed to parse");

    for px in [256, 512] {
        fs::write(out_dir.join(format!("AppIcon-{px}.png")), render_icon_png(&tree, px)).unwrap();
    }

    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let images: Vec<Vec<u8>> = sizes.iter().map(|&px| render_icon_png(&tree, px)).collect();
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&px, img) in sizes.iter().zip(&images) {
        let dim = if px >= 256 { 0 } else { px as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        ico.extend_from_slice(&(img.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in &images {
        ico.extend_from_slice(img);
    }
    let ico_path = out_dir.join("AppIcon.ico");
    fs::write(&ico_path, ico).unwrap();

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_resources(&ico_path);
    }
}

#[cfg(windows)]
fn embed_windows_resources(ico_path: &PathBuf) {
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico_path.to_str().unwrap());
    res.set("ProductName", "Arthur's Universe Playground");
    res.set("FileDescription", "Arthur's Universe Playground");
    res.compile().expect("failed to embed Windows resources");
}

#[cfg(not(windows))]
fn embed_windows_resources(_ico_path: &PathBuf) {
    println!("cargo:warning=cross-compiling for Windows: app icon not embedded in the .exe");
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    rasterise_logo(&out_dir);
    platform_icons(&out_dir);
    let dir = PathBuf::from("shaders");
    println!("cargo:rerun-if-changed=shaders");
    let globals = fs::read_to_string(dir.join("globals.wgsl")).unwrap();
    let common = fs::read_to_string(dir.join("common.wgsl")).unwrap();

    for scene in ["planet", "ring", "particle", "line", "sky", "solid"] {
        let body = fs::read_to_string(dir.join(format!("{scene}.wgsl"))).unwrap();
        let src = format!("{globals}\n{common}\n{body}");
        compile(scene, &src, &out_dir);
    }
    let composite = fs::read_to_string(dir.join("composite.wgsl")).unwrap();
    compile("composite", &format!("{globals}\n{composite}"), &out_dir);
}
