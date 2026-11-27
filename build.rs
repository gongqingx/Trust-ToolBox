use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_gnu_windres() -> Option<PathBuf> {
    let preferred = Path::new("C:/Users/qin/AppData/Local/Programs/llvm-mingw/bin/x86_64-w64-mingw32-windres.exe");
    if preferred.is_file() {
        return Some(preferred.to_path_buf());
    }
    for name in [
        "x86_64-w64-mingw32-windres",
        "llvm-windres",
        "windres",
    ] {
        if Command::new(name).arg("--version").output().is_ok() {
            return Some(PathBuf::from(name));
        }
    }
    None
}

fn find_msvc_rc() -> Option<PathBuf> {
    for name in ["rc", "llvm-windres"] {
        if Command::new(name).arg("/?").output().is_ok()
            || Command::new(name).arg("--version").output().is_ok()
        {
            return Some(PathBuf::from(name));
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app_icon.ico");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR missing"));
    let assets_dir = manifest_dir.join("assets");
    let rc_path = assets_dir.join("app.rc");

    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "gnu" {
        let obj_path = out_dir.join("app_icon.o");
        let windres = find_gnu_windres().expect("GNU windres not found to embed assets/app_icon.ico");
        let status = Command::new(&windres)
            .arg("-I")
            .arg(&assets_dir)
            .arg("-i")
            .arg(&rc_path)
            .arg("-o")
            .arg(&obj_path)
            .arg("-O")
            .arg("coff")
            .status()
            .expect("failed to execute windres");
        assert!(status.success(), "windres failed with status {status}");
        println!("cargo:rustc-link-arg={}", obj_path.display());
    } else if target_env == "msvc" {
        let Some(rc_tool) = find_msvc_rc() else {
            println!("cargo:warning=rc.exe/llvm-windres not found; skipping Windows icon resource embedding");
            return;
        };
        let is_llvm = rc_tool
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.contains("windres"));
        let res_path = out_dir.join("app_icon.res");
        let status = if is_llvm {
            Command::new(&rc_tool)
                .arg("-I")
                .arg(&assets_dir)
                .arg("-i")
                .arg(&rc_path)
                .arg("-o")
                .arg(&res_path)
                .arg("-O")
                .arg("res")
                .status()
        } else {
            Command::new(&rc_tool)
                .arg("/nologo")
                .arg("/i")
                .arg(&assets_dir)
                .arg("/fo")
                .arg(&res_path)
                .arg(&rc_path)
                .status()
        };
        match status {
            Ok(st) if st.success() => {
                println!("cargo:rustc-link-arg={}", res_path.display());
            }
            Ok(st) => println!("cargo:warning=resource compiler exited with {st}"),
            Err(e) => println!("cargo:warning=failed to run resource compiler: {e}"),
        }
    }
}
