// SPDX-License-Identifier: MIT
//! Compiles the planegcs shim, when the bench is asked to include planegcs.
//!
//! Off by default. planegcs has to be built first — `tools/build-planegcs.sh`
//! does that from a pinned FreeCAD release — and its location given in
//! `FCAD_PLANEGCS_DIR`. Nothing is fetched from here: a build that reaches the
//! network without being asked to is a build nobody can reproduce offline.

// A build script has no caller to return an error to; panicking is how cargo
// is told a build cannot proceed, and the message is what the person sees.
#![allow(
    clippy::panic,
    reason = "a build script reports failure by panicking; there is no caller"
)]

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=planegcs-bridge/planegcs_shim.cpp");
    println!("cargo:rerun-if-changed=planegcs-bridge/planegcs_shim.h");
    println!("cargo:rerun-if-env-changed=FCAD_PLANEGCS_DIR");
    println!("cargo:rerun-if-env-changed=FCAD_EIGEN_INCLUDE");
    println!("cargo:rerun-if-env-changed=FCAD_BOOST_INCLUDE");
    println!("cargo:rerun-if-env-changed=CXX");
    println!("cargo:rerun-if-env-changed=AR");

    if std::env::var_os("CARGO_FEATURE_PLANEGCS").is_none() {
        return;
    }

    // Absent rather than fatal, the same way a build without Open CASCADE
    // still produces a crate. `--all-features` has to work on a machine that
    // has never built planegcs, and the bench skips the candidate at run time
    // instead of the build failing at a distance from the cause.
    println!("cargo::rustc-check-cfg=cfg(planegcs_linked)");
    let Some(planegcs) = std::env::var_os("FCAD_PLANEGCS_DIR").map(PathBuf::from) else {
        println!(
            "cargo::warning=the planegcs feature is on but FCAD_PLANEGCS_DIR is not set, so \
             that candidate will be skipped; build it with tools/build-planegcs.sh"
        );
        return;
    };
    let tree = planegcs.join("tree");
    if !tree.join("App/planegcs/GCS.h").exists() {
        println!(
            "cargo::warning={} does not look like a planegcs build; that candidate will be \
             skipped",
            planegcs.display()
        );
        return;
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("cargo sets target OS");
    if target_os != "macos" && target_os != "linux" {
        println!(
            "cargo::warning=the linked planegcs lab path currently supports macOS and Linux, not \
             {target_os}; that candidate will be skipped"
        );
        return;
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let object = out.join("planegcs_shim.o");
    let compiler = std::env::var("CXX").unwrap_or_else(|_| "c++".to_owned());

    let mut command = Command::new(&compiler);
    command
        .args(["-std=c++17", "-O2", "-fPIC", "-w", "-c"])
        .arg("planegcs-bridge/planegcs_shim.cpp")
        .arg("-Iplanegcs-bridge")
        .arg(format!("-I{}", tree.join("App/planegcs").display()))
        .arg(format!("-I{}", tree.display()))
        .arg("-o")
        .arg(&object);
    for include in ["FCAD_EIGEN_INCLUDE", "FCAD_BOOST_INCLUDE"] {
        if let Some(path) = std::env::var_os(include) {
            command.arg(format!("-I{}", PathBuf::from(path).display()));
        }
    }
    for candidate in [
        "/opt/homebrew/include/eigen3",
        "/usr/local/include/eigen3",
        "/usr/include/eigen3",
        "/opt/homebrew/include",
        "/usr/local/include",
        "/usr/include",
    ] {
        if PathBuf::from(candidate).exists() {
            command.arg(format!("-I{candidate}"));
        }
    }

    let status = command.status().expect("a C++ compiler is available");
    assert!(status.success(), "the planegcs shim did not compile");
    println!("cargo::rustc-cfg=planegcs_linked");

    // Bundled into a static archive of FerriteCAD's own code; planegcs itself
    // stays a shared library beside it, which is the point.
    let archive = out.join("libplanegcs_shim.a");
    let archiver = std::env::var("AR").unwrap_or_else(|_| "ar".to_owned());
    let status = Command::new(archiver)
        .arg("crs")
        .arg(&archive)
        .arg(&object)
        .status()
        .expect("ar is available");
    assert!(status.success(), "could not archive the planegcs shim");

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=planegcs_shim");
    println!("cargo:rustc-link-search=native={}", planegcs.display());
    println!("cargo:rustc-link-lib=dylib=planegcs");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", planegcs.display());

    // The C++ standard library, which the shim and planegcs both need.
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
}
