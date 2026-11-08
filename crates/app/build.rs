//! Embeds the application icon into the Pastebox executable.
//!
//! Cargo has no key for Win32 resources, so the `.rc` is compiled here and the
//! resulting object is handed to the linker. No crates are added: MinGW's
//! `windres` covers every `*-windows-gnu` target and the Windows SDK's `rc.exe`
//! covers `*-msvc`. Non-Windows hosts return immediately, so a plain `cargo
//! test` stays unaffected.
//!
//! Build scripts run with the package root (`crates/app`) as the working
//! directory, hence the `../../assets` paths.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const ASSETS: &str = "../../assets";

fn main() {
    println!("cargo:rerun-if-changed={ASSETS}/pastebox.rc");
    println!("cargo:rerun-if-changed={ASSETS}/icon.ico");

    let target = env::var("TARGET").unwrap_or_default();
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let rc = Path::new(ASSETS).join("pastebox.rc");

    let linked = if target.contains("gnu") {
        // MinGW names its resource compiler after the toolchain triple, which is
        // not the Rust target triple in general (x86_64-pc-windows-gnu ships as
        // x86_64-w64-mingw32-windres), so try each documented spelling.
        let arch = target.split('-').next().unwrap_or("x86_64");
        let triple = format!("{arch}-w64-mingw32");
        let obj = out_dir.join("pastebox-res.o");
        let args = ["-I", ASSETS, "-O", "coff"];
        [
            format!("{target}-windres"),
            format!("{triple}-windres"),
            "windres".to_string(),
        ]
        .iter()
        .find_map(|tool| compile(tool, &rc, &obj, "-o", &args).map(|()| obj.clone()))
    } else {
        let res = out_dir.join("pastebox-res.res");
        let args = ["/nologo", "/I", ASSETS];
        compile("rc.exe", &rc, &res, "/fo", &args)
            .or_else(|| compile("rc", &rc, &res, "/fo", &args))
            .or_else(|| {
                // rc.exe is only on PATH inside a Visual Studio developer prompt;
                // the GitHub windows-latest shell is not one. Look where the
                // Windows SDK actually installs it (newest version first).
                find_sdk_rc().and_then(|tool| {
                    let tool = tool.to_string_lossy().to_string();
                    compile(&tool, &rc, &res, "/fo", &args)
                })
            })
            .map(|()| res)
    };

    match linked {
        Some(path) => println!("cargo:rustc-link-arg={}", path.display()),
        None => {
            // An exe without its icon shows the generic page glyph everywhere;
            // fail the Windows build instead of shipping that silently.
            panic!(
                "no resource compiler found for target {target}: cannot embed {}. \
                 Install the Windows SDK, or build the *-windows-gnu target with MinGW's windres.",
                rc.display()
            );
        }
    }
}

/// Locates `rc.exe` in the Windows SDK (e.g.
/// `C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\rc.exe`).
fn find_sdk_rc() -> Option<PathBuf> {
    let roots = [
        r"C:\Program Files (x86)\Windows Kits\10\bin",
        r"C:\Program Files\Windows Kits\10\bin",
    ];
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        let mut versions: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        versions.sort();
        for version in versions.iter().rev() {
            for arch in ["x64", "x86"] {
                let candidate = version.join(arch).join("rc.exe");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn compile(tool: &str, rc: &Path, out: &Path, out_flag: &str, extra: &[&str]) -> Option<()> {
    let status = Command::new(tool)
        .args(extra)
        .arg(out_flag)
        .arg(out)
        .arg(rc)
        .status()
        .ok()?;
    status.success().then_some(())
}
