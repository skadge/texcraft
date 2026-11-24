//! `cargo xtask retex-wasm`: retex's engine as a WebAssembly module
//! (`wasm32-wasip1`), built from the crates.io release `Cargo.lock` pins,
//! for the browser build (docs/ROADMAP.md). Written to
//! `<target>/retex-wasm/retex.wasm`, with the version beside it.
//!
//! It is built as retex's own `cargo xtask wasm` builds it, from the
//! published crate and its lock file:
//!
//! * The Lua retex bundles is C, compiled with wasi-sdk's clang
//!   (`WASI_SDK_PATH`), whose `libsetjmp` turns Lua's `setjmp`/`longjmp`
//!   into WebAssembly exceptions.
//! * TeX's end of a job unwinds through the engine: Rust's unwinding on
//!   WebAssembly needs the standard library rebuilt with `panic=unwind`
//!   (nightly with `rust-src` and the `wasm32-wasip1` target).
//! * Both use the "legacy" encoding of exceptions, which browsers of the
//!   last years run.
//! * WASI has no threads, so the module's own stack is made large (the
//!   engine's recursion).
//! * No package downloads: retex's `download` feature is off.
//!
//! `RETEX_WASM_TOOLCHAIN` names the nightly toolchain (default `nightly`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The module's stack: 128 MiB.
const STACK: usize = 128 << 20;
const EXCEPTIONS_C: &str = "-mllvm -wasm-enable-sjlj -mexception-handling -mllvm -wasm-use-legacy-eh=true";

/// The version of the `retex` package in `Cargo.lock`.
pub(crate) fn pinned_version(root: &Path) -> Result<String, String> {
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).map_err(|e| format!("Cargo.lock: {e}"))?;
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line == "name = \"retex\"" {
            let version = lines.next().and_then(|l| l.strip_prefix("version = \"")?.strip_suffix('"'));
            return version.map(str::to_owned).ok_or_else(|| "Cargo.lock: retex has no version line".into());
        }
    }
    Err("Cargo.lock has no retex package".into())
}

pub fn run(root: &Path) -> Result<(), String> {
    let sdk = PathBuf::from(std::env::var_os("WASI_SDK_PATH").ok_or("set WASI_SDK_PATH to a wasi-sdk directory (wasi-sdk 34 or later)")?);
    let sysroot = sdk.join("share/wasi-sysroot");
    if !sdk.join("bin/clang").is_file() || !sysroot.is_dir() {
        return Err(format!("{}: not a wasi-sdk directory", sdk.display()));
    }
    let version = pinned_version(root)?;
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
    let out = target.join("retex-wasm");
    let stamp = out.join("version.txt");
    let module = out.join("retex.wasm");
    if module.is_file() && std::fs::read_to_string(&stamp).ok().as_deref() == Some(version.as_str()) {
        eprintln!("{}: retex {version}, up to date", module.display());
        return Ok(());
    }
    let rustflags = format!(
        "-L {} -C link-arg=-zstack-size={STACK} -C link-arg=--export=retex_resume -C panic=unwind \
         -C target-feature=+exception-handling -C llvm-args=-wasm-use-legacy-eh=true",
        sysroot.join("lib/wasm32-wasip1").display()
    );
    let install = out.join("install");
    // A dated nightly makes the build repeatable (CI pins one).
    let toolchain = format!("+{}", std::env::var("RETEX_WASM_TOOLCHAIN").unwrap_or_else(|_| "nightly".into()));
    let args = [
        &toolchain,
        "install",
        "--locked",
        "--force",
        "--root",
        install.to_str().ok_or("the target directory's path is not UTF-8")?,
        &format!("retex@={version}"),
        "--no-default-features",
        "--target",
        "wasm32-wasip1",
        "-Zbuild-std=std,panic_unwind",
    ];
    eprintln!("$ cargo {}", args.join(" "));
    let ok = Command::new("cargo")
        .args(args)
        .env("CARGO_TARGET_DIR", out.join("build"))
        .env("CC_wasm32_wasip1", sdk.join("bin/clang"))
        .env("AR_wasm32_wasip1", sdk.join("bin/llvm-ar"))
        .env("CFLAGS_wasm32_wasip1", format!("--sysroot={} {EXCEPTIONS_C}", sysroot.display()))
        .env("CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS", rustflags)
        // rustup's toolchain override of this repository must not apply.
        .env_remove("RUSTUP_TOOLCHAIN")
        .status()
        .map_err(|e| format!("cargo +nightly: {e}"))?
        .success();
    if !ok {
        return Err("building retex for wasm32-wasip1 failed (nightly with rust-src and wasm32-wasip1 needed)".into());
    }
    let status = Command::new(sdk.join("bin/llvm-strip"))
        .arg(install.join("bin/retex.wasm"))
        .arg("-o")
        .arg(&module)
        .status()
        .map_err(|e| format!("llvm-strip: {e}"))?;
    if !status.success() {
        return Err("llvm-strip failed".into());
    }
    std::fs::write(&stamp, &version).map_err(|e| format!("{}: {e}", stamp.display()))?;
    let size = std::fs::metadata(&module).map_err(|e| format!("{}: {e}", module.display()))?.len();
    eprintln!("{}: retex {version}, {:.1} MB", module.display(), size as f64 / 1e6);
    Ok(())
}
