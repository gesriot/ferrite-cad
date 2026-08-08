// SPDX-License-Identifier: MIT
//! Builds the C++ bridge, if Open CASCADE is present.
//!
//! Open CASCADE is not a Rust dependency and cannot be fetched, so a checkout
//! on a machine without it must still build. This script therefore treats OCCT
//! as optional: found, and the adapter is compiled; absent, and the crate
//! compiles to a stub whose constructor says why it cannot work.
//!
//! Silent degradation is the risk in that arrangement, so it is made loud in
//! the one place it matters. Setting `FERRITECAD_REQUIRE_OCCT=1` turns a
//! missing kernel into a build failure, and the pin workflow sets it — a run
//! whose job is to prove the adapter works cannot pass by skipping it.

// A build script has no caller to return an error to; panicking is how cargo
// is told the build failed, and the message is what the developer reads.
#![allow(
    clippy::panic,
    reason = "the documented way for a build script to fail"
)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo::rerun-if-changed=../ferritecad-occt-bridge");
    println!("cargo::rerun-if-env-changed=OpenCASCADE_DIR");
    println!("cargo::rerun-if-env-changed=CMAKE_PREFIX_PATH");
    println!("cargo::rerun-if-env-changed=FERRITECAD_REQUIRE_OCCT");
    println!("cargo::rustc-check-cfg=cfg(occt)");

    let required = env::var("FERRITECAD_REQUIRE_OCCT").as_deref() == Ok("1");

    match build_bridge() {
        Ok(()) => println!("cargo::rustc-cfg=occt"),
        Err(reason) if required => {
            panic!(
                "FERRITECAD_REQUIRE_OCCT=1 was set, so a missing Open CASCADE is a failure \
                 rather than a skipped adapter.\n{reason}"
            );
        }
        Err(reason) => {
            println!(
                "cargo::warning=Open CASCADE was not usable, so ferritecad-occt is being built \
                 without a kernel and OcctKernel::new will refuse. Reason: {reason}"
            );
        }
    }
}

fn build_bridge() -> Result<(), String> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(|e| e.to_string())?);
    let build_dir = out_dir.join("bridge-build");
    let source_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?)
        .join("..")
        .join("ferritecad-occt-bridge");

    if !source_dir.join("CMakeLists.txt").is_file() {
        return Err(format!("no bridge sources at {}", source_dir.display()));
    }

    // No -G: the platform default is whichever toolchain is installed, which
    // is the lesson the pin workflow paid for. Naming a Visual Studio release
    // here would break on the next one.
    let mut configure = Command::new("cmake");
    configure
        .arg("-S")
        .arg(&source_dir)
        .arg("-B")
        .arg(&build_dir)
        .arg("-DCMAKE_BUILD_TYPE=Release");
    if let Ok(occt) = env::var("OpenCASCADE_DIR") {
        configure.arg(format!("-DOpenCASCADE_DIR={occt}"));
    }
    run(configure, "configuring the bridge")?;

    let mut build = Command::new("cmake");
    build
        .arg("--build")
        .arg(&build_dir)
        .arg("--config")
        .arg("Release");
    run(build, "building the bridge")?;

    let info = read_info(&build_dir)?;
    let bridge_dir = info
        .get("bridge_dir")
        .ok_or("the bridge did not report where it was built")?;
    let occt_dir = info
        .get("occt_library_dir")
        .ok_or("the bridge did not report where Open CASCADE lives")?;

    println!("cargo::rustc-link-search=native={bridge_dir}");
    println!("cargo::rustc-link-lib=dylib=ferritecad_occt_bridge");

    // The bridge links Open CASCADE itself, so Rust needs only to be able to
    // find the bridge at run time. RPATH rather than an environment variable:
    // on macOS SIP strips DYLD_* when a protected shell spawns a process,
    // which is exactly where the variable looks like it should work.
    if cfg!(unix) {
        println!("cargo::rustc-link-arg=-Wl,-rpath,{bridge_dir}");
        println!("cargo::rustc-link-arg=-Wl,-rpath,{occt_dir}");
    } else {
        // Windows has no RPATH; the loader searches PATH.
        println!(
            "cargo::warning=add {bridge_dir} and {occt_dir} to PATH before running \
             ferritecad-occt tests"
        );
    }

    Ok(())
}

fn run(mut command: Command, what: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("{what}: could not run cmake ({e})"))?;

    if output.status.success() {
        return Ok(());
    }

    // The tail is where CMake puts the reason; the head is boilerplate.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail: Vec<&str> = stderr
        .lines()
        .chain(stdout.lines())
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(6)
        .collect();

    Err(format!(
        "{what} failed ({}): {}",
        output.status,
        detail.into_iter().rev().collect::<Vec<_>>().join(" | ")
    ))
}

/// Reads the key/value file CMake generated, whichever configuration produced
/// it.
fn read_info(build_dir: &Path) -> Result<std::collections::BTreeMap<String, String>, String> {
    let entries = std::fs::read_dir(build_dir)
        .map_err(|e| format!("cannot read the bridge build directory: {e}"))?;

    let mut candidates: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("ferritecad-bridge-info-") && name.ends_with(".txt")
                })
        })
        .collect();
    candidates.sort();

    let chosen = candidates
        .last()
        .ok_or("the bridge build produced no information file")?;
    let text = std::fs::read_to_string(chosen)
        .map_err(|e| format!("cannot read {}: {e}", chosen.display()))?;

    Ok(text
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect())
}
