// SPDX-License-Identifier: MIT
fn main() -> ferritecad_types::Result<()> {
    let directory = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .expect("output directory argument");
    std::fs::create_dir_all(&directory)?;
    for (name, radius) in [("broken.fcad", 25.0), ("healthy.fcad", 3.0)] {
        let path = directory.join(name);
        if name == "broken.fcad" {
            ferritecad_fixtures::write_constrained_fillet_chain(&path, radius, 20.0, 20.0)?;
        } else {
            ferritecad_fixtures::write_constrained_fillet_chain(&path, radius, 60.0, 40.0)?;
        }
        println!("{}", path.display());
    }
    Ok(())
}
