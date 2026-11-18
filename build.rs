use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/app.manifest");
    println!("cargo:rerun-if-changed=packaging/windows/desklink.rc");
    println!("cargo:rerun-if-env-changed=DESKLINK_WINDRES");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var_os("CARGO_FEATURE_DESKTOP").is_none()
    {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bin=desklink=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=desklink=/MANIFESTINPUT:{}",
            root.join("packaging/windows/app.manifest").display()
        );
    } else {
        let compiler =
            env::var("DESKLINK_WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
        let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("desklink-resource.o");
        let status = Command::new(compiler)
            .arg("-I")
            .arg(root.join("packaging/windows"))
            .arg(root.join("packaging/windows/desklink.rc"))
            .arg("-O")
            .arg("coff")
            .arg("-o")
            .arg(&output)
            .status()
            .expect("Install target windres or set DESKLINK_WINDRES");
        assert!(
            status.success(),
            "Windows manifest/resource compilation failed"
        );
        println!("cargo:rustc-link-arg-bin=desklink={}", output.display());
    }
}
