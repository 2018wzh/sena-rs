use shinki_emu_build::{write_core_build_manifest, BuildAsset, CoreBuildManifest};
use std::{env, path::PathBuf};

fn main() {
    for name in ["HOST", "TARGET", "PROFILE"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    let mut manifest = CoreBuildManifest::new("sena-rs", "rust-cdylib")
        .expect("Cargo must provide the core build target and profile");
    assert_eq!(manifest.target, "x86_64-pc-windows-msvc",
        "sena-rs embedded adapter currently supports only x86_64-pc-windows-msvc");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("locating the fork source root");
    manifest.licenses = vec![
        BuildAsset { source: root.join("LICENSE-MPL-2.0"), dest: "licenses/sena-rs/LICENSE-MPL-2.0".into() },
    ];
    write_core_build_manifest(&manifest).expect("writing sena-rs core-build.json");
}
