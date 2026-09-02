// SPDX-License-Identifier: MIT
//! Writes the production FBX bytes the independent readers are pointed at.
//!
//! Not a command and not a route to one: §22B-1b2 deliberately has no
//! `export-fbx`, and the gates outside Rust still need real writer output to
//! read. This produces exactly that, from the same measured scene the Rust
//! gate uses, into a directory a gate script owns.

// The measured scene is defined once, beside the gate that asserts on it.
#[path = "../tests/fbx_scene/mod.rs"]
mod fbx_scene;

use std::io::BufWriter;
use std::path::PathBuf;

use ferritecad_export::write_fbx_ascii_7400;

fn main() -> std::process::ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let Some(directory) = arguments.next().map(PathBuf::from) else {
        eprintln!("usage: fbx_gate_artefacts OUTPUT_DIRECTORY");
        return std::process::ExitCode::from(2);
    };
    if arguments.next().is_some() {
        eprintln!("usage: fbx_gate_artefacts OUTPUT_DIRECTORY");
        return std::process::ExitCode::from(2);
    }

    let scenes = [
        ("fcad-measured.fbx", fbx_scene::measured_scene()),
        ("fcad-escaping.fbx", fbx_scene::escaping_scene()),
    ];
    for (name, scene) in scenes {
        let path = directory.join(name);
        let file = match std::fs::File::create(&path) {
            Ok(file) => file,
            Err(error) => {
                eprintln!("cannot write {}: {error}", path.display());
                return std::process::ExitCode::FAILURE;
            }
        };
        let mut sink = BufWriter::new(file);
        let report = match write_fbx_ascii_7400(&scene, &mut sink) {
            Ok(report) => report,
            Err(error) => {
                eprintln!("cannot write {name}: {error}");
                return std::process::ExitCode::FAILURE;
            }
        };
        if let Err(error) = std::io::Write::flush(&mut sink) {
            eprintln!("cannot finish {name}: {error}");
            return std::process::ExitCode::FAILURE;
        }
        println!(
            "{name} bytes={} models={} geometries={} materials={} complete={}",
            report.bytes(),
            report.models(),
            report.geometries(),
            report.materials(),
            report.is_complete()
        );
    }
    std::process::ExitCode::SUCCESS
}
