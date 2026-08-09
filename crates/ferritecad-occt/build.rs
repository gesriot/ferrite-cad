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

    // The cache key must change when the code or toolchain that produced a
    // cached result changes. `bridge 0.0.1` did not: the crate version moves
    // on releases, not on edits to the C++, so a changed algorithm would have
    // gone on serving results computed by the old one. A source-only digest
    // still misses compiler and flag changes, both of which KernelIdentity's
    // contract explicitly treats as build identity.
    //
    // Comment-only edits invalidate the cache too. That over-invalidates, and
    // the cost is a rebuild; the alternative under-invalidates, and the cost
    // is a wrong answer served quickly.
    let target = env::var("TARGET").map_err(|e| format!("Cargo did not provide TARGET: {e}"))?;
    let determinants = [
        ("target", target.as_str()),
        ("configuration", required_info(&info, "configuration")?),
        ("generator", required_info(&info, "generator")?),
        ("cxx_compiler_id", required_info(&info, "cxx_compiler_id")?),
        (
            "cxx_compiler_version",
            required_info(&info, "cxx_compiler_version")?,
        ),
        (
            "cxx_compiler_target",
            info.get("cxx_compiler_target").map_or("", String::as_str),
        ),
        (
            "cxx_flags",
            info.get("cxx_flags").map_or("", String::as_str),
        ),
        (
            "cxx_flags_release",
            info.get("cxx_flags_release").map_or("", String::as_str),
        ),
    ];
    let fingerprint = fingerprint_build(&source_dir, &determinants)?;
    println!("cargo::rustc-env=FERRITECAD_BRIDGE_BUILD=bridge {fingerprint} {target}");

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

    // RPATH rather than an environment variable for this package's own
    // executables: on macOS SIP can strip DYLD_* at protected-process
    // boundaries. Cargo does not propagate this package-local link argument
    // to downstream binaries; the pin workflow supplies the raw library path
    // when it launches the unbundled CLI. Windows has no RPATH and searches
    // PATH instead, which the pin workflow sets.
    if cfg!(unix) {
        println!("cargo::rustc-link-arg=-Wl,-rpath,{occt_dir}");
    }

    Ok(())
}

/// A digest of every bridge source file and build determinant, in stable order.
///
/// Deliberately covers the CMake file as well: a change to the build flags can
/// change the geometry as surely as a change to the code.
fn fingerprint_build(source_dir: &Path, determinants: &[(&str, &str)]) -> Result<String, String> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_sources(source_dir, &mut files)?;
    files.sort();

    if files.is_empty() {
        return Err("the bridge has no sources to fingerprint".to_owned());
    }

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"ferritecad-bridge-build-v1\0");
    for file in &files {
        let name = file
            .strip_prefix(source_dir)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        hasher.update(name.as_bytes());
        hasher.update(b"\0");

        let bytes =
            std::fs::read(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);

        println!("cargo::rerun-if-changed={}", file.display());
    }

    for (name, value) in determinants {
        hasher.update(&(name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
        hasher.update(&(value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }

    // Keep the full BLAKE3-256 value. Truncating to sixteen hex characters
    // turns a correctness boundary into a 64-bit identifier for no practical
    // saving: this string appears once in a cache key, not once per triangle.
    Ok(hasher.finalize().to_hex().to_string())
}

fn required_info<'a>(
    info: &'a std::collections::BTreeMap<String, String>,
    key: &str,
) -> Result<&'a str, String> {
    info.get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("the bridge did not report {key}"))
}

fn collect_sources(dir: &Path, into: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;

    for entry in entries {
        let path = entry
            .map_err(|e| format!("cannot read an entry of {}: {e}", dir.display()))?
            .path();
        if path.is_dir() {
            collect_sources(&path, into)?;
        } else {
            let keep = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e, "cpp" | "h" | "hpp" | "cxx" | "hxx" | "cmake"))
                || path.file_name().and_then(|n| n.to_str()) == Some("CMakeLists.txt");
            if keep {
                into.push(path);
            }
        }
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

/// Reads the key/value file for the configuration the build command selected.
fn read_info(build_dir: &Path) -> Result<std::collections::BTreeMap<String, String>, String> {
    // Both configure and build above select Release. A multi-config generator
    // may generate files for several configurations at once; choosing one by
    // directory or lexical order can therefore key a Release binary with
    // another configuration's flags.
    let chosen = build_dir.join("ferritecad-bridge-info-Release.txt");
    let text = std::fs::read_to_string(&chosen).map_err(|e| {
        format!(
            "cannot read the Release bridge information at {}: {e}",
            chosen.display()
        )
    })?;

    Ok(text
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect())
}
