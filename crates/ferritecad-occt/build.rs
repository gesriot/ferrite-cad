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

    // The shim is static and ends up inside the Rust binary; Open CASCADE
    // stays dynamic behind it, which is where the licence policy applies.
    println!("cargo::rustc-link-search=native={bridge_dir}");
    println!("cargo::rustc-link-lib=static=ferritecad_occt_bridge");

    println!("cargo::rustc-link-search=native={occt_dir}");
    let toolkits = info
        .get("occt_libraries")
        .ok_or("the bridge did not report which Open CASCADE toolkits it uses")?;
    for toolkit in toolkits.split(',').filter(|t| !t.trim().is_empty()) {
        println!("cargo::rustc-link-lib=dylib={}", toolkit.trim());
    }

    // A static C++ library dragged into a Rust link needs the C++ runtime
    // named explicitly; rustc only assumes a C one. MSVC picks its runtime up
    // from the object files, so Windows needs nothing here.
    if cfg!(target_os = "macos") {
        println!("cargo::rustc-link-lib=dylib=c++");
    } else if cfg!(target_os = "linux") {
        println!("cargo::rustc-link-lib=dylib=stdc++");
    }

    // RPATH rather than an environment variable: on macOS SIP strips DYLD_*
    // when a protected shell spawns a process, which is exactly where the
    // variable looks like it should work. Windows has no RPATH and searches
    // PATH instead, which the pin workflow sets.
    if cfg!(unix) {
        println!("cargo::rustc-link-arg=-Wl,-rpath,{occt_dir}");
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

    // Show the compiler's own diagnostics, not the tail of the output.
    //
    // The first version of this reported the last few lines, which for a
    // failed C++ build are make's "*** Error 1" summary — three lines saying
    // that something failed and none saying what. The real message is well
    // above them, so it is searched for by name.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined: Vec<&str> = stderr
        .lines()
        .chain(stdout.lines())
        .filter(|line| !line.trim().is_empty())
        .collect();

    let diagnostics: Vec<&str> = combined
        .iter()
        .copied()
        .filter(|line| {
            let lowered = line.to_ascii_lowercase();
            lowered.contains("error")
                && !lowered.contains("error 1")
                && !lowered.contains("error 2")
        })
        .take(8)
        .collect();

    let detail = if diagnostics.is_empty() {
        // Nothing named itself an error, so the tail is the best available.
        combined
            .iter()
            .rev()
            .take(8)
            .rev()
            .copied()
            .collect::<Vec<_>>()
    } else {
        diagnostics
    };

    Err(format!(
        "{what} failed ({}):\n  {}",
        output.status,
        detail.join("\n  ")
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
