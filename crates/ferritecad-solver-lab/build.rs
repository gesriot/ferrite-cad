// SPDX-License-Identifier: MIT
//! Compiles the planegcs shim and links the shared library, when asked to.
//!
//! Off by default. planegcs has to be built first – `tools/build-planegcs.sh`
//! does that from a pinned FreeCAD release on Linux, macOS and Windows – and
//! its location given in `FCAD_PLANEGCS_DIR`. Nothing is fetched from here: a
//! build that reaches the network without being asked to is a build nobody can
//! reproduce offline.
//!
//! Absent is not fatal by default, the same way a build without Open CASCADE
//! still produces a crate: `--all-features` has to work on a machine that has
//! never built planegcs. Silent degradation is the risk in that arrangement,
//! so `FERRITECAD_REQUIRE_PLANEGCS=1` turns every reason to skip into a build
//! failure, and the pin workflow sets it. A run whose job is to prove planegcs
//! works cannot pass by not having it.

// A build script has no caller to return an error to; panicking is how cargo
// is told a build cannot proceed, and the message is what the person sees.
#![allow(
    clippy::panic,
    reason = "a build script reports failure by panicking; there is no caller"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=planegcs-bridge");
    println!("cargo:rerun-if-changed=../../tools/planegcs/pin.env");
    println!("cargo:rerun-if-env-changed=FCAD_PLANEGCS_DIR");
    println!("cargo:rerun-if-env-changed=FCAD_EIGEN_INCLUDE");
    println!("cargo:rerun-if-env-changed=FCAD_BOOST_INCLUDE");
    println!("cargo:rerun-if-env-changed=FERRITECAD_REQUIRE_PLANEGCS");
    println!("cargo::rustc-check-cfg=cfg(planegcs_linked)");

    let required = std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref() == Ok("1");

    if std::env::var_os("CARGO_FEATURE_PLANEGCS").is_none() {
        assert!(
            !required,
            "FERRITECAD_REQUIRE_PLANEGCS=1 was set, so building the lab without the planegcs \
             feature is a failure rather than a build with one candidate. Add --features planegcs."
        );
        return;
    }

    // Emitted before anything can go wrong, because the crate compiles it in
    // whether or not the library turned up, and a test that cannot name what
    // it expected cannot report a substitution.
    match expected_provenance() {
        Ok(provenance) => {
            println!("cargo::rustc-env=FCAD_PLANEGCS_EXPECTED_PROVENANCE={provenance}");
        }
        Err(reason) => panic!("the planegcs pin could not be read: {reason}"),
    }

    match link_planegcs() {
        Ok(()) => println!("cargo::rustc-cfg=planegcs_linked"),
        Err(reason) if required => panic!(
            "FERRITECAD_REQUIRE_PLANEGCS=1 was set, so an unlinked planegcs is a failure rather \
             than a skipped candidate.\n{reason}"
        ),
        Err(reason) => println!(
            "cargo::warning=planegcs was not linked, so that candidate will be skipped. Reason: \
             {reason}"
        ),
    }
}

/// What the library must say about itself, read from the one pinned file.
///
/// Kept here rather than written out as a Rust constant so that the string the
/// library was built to return and the string the lab expects to hear cannot
/// drift apart: both are this file, read once by the build that makes the
/// library and once by the build that uses it.
fn expected_provenance() -> Result<String, String> {
    let manifest =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").map_err(|error| error.to_string())?);
    let pin = manifest.join("../../tools/planegcs/pin.env");
    let text = std::fs::read_to_string(&pin)
        .map_err(|error| format!("cannot read {}: {error}", pin.display()))?;

    let mut values = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once('=') {
            values.insert(name.trim().to_owned(), value.trim().to_owned());
        }
    }

    let tag = values
        .get("FCAD_PLANEGCS_FREECAD_TAG")
        .ok_or("no FCAD_PLANEGCS_FREECAD_TAG in the pin")?;
    let digest = values
        .get("FCAD_PLANEGCS_ARCHIVE_SHA256")
        .ok_or("no FCAD_PLANEGCS_ARCHIVE_SHA256 in the pin")?;
    // The same sentence tools/build-planegcs.sh compiles into the library.
    Ok(format!(
        "planegcs from FreeCAD {tag}, archive SHA-256 {digest}"
    ))
}

fn link_planegcs() -> Result<(), String> {
    let Some(planegcs) = std::env::var_os("FCAD_PLANEGCS_DIR").map(PathBuf::from) else {
        return Err(
            "FCAD_PLANEGCS_DIR is not set; build it with tools/build-planegcs.sh".to_owned(),
        );
    };
    let tree = planegcs.join("tree");
    if !tree.join("App/planegcs/GCS.h").exists() {
        return Err(format!(
            "{} does not look like a planegcs build: no tree/App/planegcs/GCS.h",
            planegcs.display()
        ));
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("cargo sets target OS");
    // Named per platform rather than searched for, so that a library under any
    // other name is an absent one and not a silent substitution. The import
    // library is linker metadata: it is what the link reads on Windows, and it
    // contains no planegcs implementation.
    let (library, link_name) = match target_os.as_str() {
        "macos" => ("libplanegcs.dylib", "planegcs"),
        "linux" => ("libplanegcs.so", "planegcs"),
        "windows" => ("planegcs.dll", "planegcs"),
        other => return Err(format!("no planegcs library name is defined for {other}")),
    };
    if !planegcs.join(library).exists() {
        return Err(format!(
            "{} has no {library}; planegcs must be a shared library beside its sources",
            planegcs.display()
        ));
    }
    if target_os == "windows" && !planegcs.join("planegcs.lib").exists() {
        return Err(format!(
            "{} has no planegcs.lib, so there is nothing for the linker to resolve the DLL \
             through",
            planegcs.display()
        ));
    }

    let bridge = build_bridge(&tree)?;

    // The shim is static and ends up inside the Rust test binary; planegcs
    // stays dynamic beside it, which is the whole point.
    println!("cargo::rustc-link-search=native={bridge}");
    println!("cargo::rustc-link-lib=static=ferritecad_planegcs_bridge");
    println!("cargo::rustc-link-search=native={}", planegcs.display());
    println!("cargo::rustc-link-lib=dylib={link_name}");

    // A static C++ library dragged into a Rust link needs the C++ runtime
    // named explicitly; rustc only assumes a C one. MSVC picks its runtime up
    // from the object files, so Windows needs nothing here.
    match target_os.as_str() {
        "macos" => println!("cargo::rustc-link-lib=dylib=c++"),
        "linux" => println!("cargo::rustc-link-lib=dylib=stdc++"),
        _ => {}
    }

    // Where the loader should look afterwards. Windows has no RPATH and
    // searches the directory of the executable and then PATH, which the pin
    // workflow sets; link.exe would reject this argument outright.
    if target_os != "windows" {
        println!("cargo::rustc-link-arg=-Wl,-rpath,{}", planegcs.display());
    }

    Ok(())
}

/// Compiles the shim, through the same cmake path the OCCT bridge uses.
fn build_bridge(tree: &Path) -> Result<String, String> {
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let build = out.join("bridge-build");
    let source = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"))
        .join("planegcs-bridge");

    // No -G: the platform default is whichever toolchain is installed, which
    // is the lesson docs/build-occt.md paid for.
    let mut configure = Command::new("cmake");
    configure
        .arg("-S")
        .arg(&source)
        .arg("-B")
        .arg(&build)
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg(format!("-DFCAD_PLANEGCS_TREE={}", tree.display()));
    for (name, candidates) in [
        (
            "FCAD_EIGEN_INCLUDE",
            [
                "/opt/homebrew/include/eigen3",
                "/usr/local/include/eigen3",
                "/usr/include/eigen3",
            ],
        ),
        (
            "FCAD_BOOST_INCLUDE",
            [
                "/opt/homebrew/include",
                "/usr/local/include",
                "/usr/include",
            ],
        ),
    ] {
        if let Some(path) = std::env::var_os(name) {
            configure.arg(format!("-D{name}={}", PathBuf::from(path).display()));
        } else if let Some(found) = candidates.iter().find(|c| Path::new(c).exists()) {
            configure.arg(format!("-D{name}={found}"));
        }
    }
    run(configure, "configuring the planegcs shim")?;

    let mut compile = Command::new("cmake");
    compile
        .arg("--build")
        .arg(&build)
        .arg("--config")
        .arg("Release");
    run(compile, "building the planegcs shim")?;

    let info = build.join("planegcs-bridge-info-Release.txt");
    let text = std::fs::read_to_string(&info)
        .map_err(|error| format!("cannot read {}: {error}", info.display()))?;
    text.lines()
        .find_map(|line| line.strip_prefix("bridge_dir="))
        .map(str::to_owned)
        .ok_or_else(|| "the shim did not report where it was built".to_owned())
}

fn run(mut command: Command, what: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("{what}: could not run {command:?}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what}: {command:?} exited with {status}"))
    }
}
