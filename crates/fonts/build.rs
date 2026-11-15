//! Slab selected profile: do not embed unreviewed optional font inputs.
use std::path::PathBuf;
fn main() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("craft_fonts.rs");
    if let Err(e) = std::fs::write(&out, "pub static CRAFT_FONTS: &[CraftFont] = &[];\n") {
        println!("cargo::error=writing {}: {e}", out.display());
    }
}
