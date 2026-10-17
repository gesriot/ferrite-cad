// SPDX-License-Identifier: MIT
use std::process::Command;

#[test]
fn dependency_probe() {
    let Ok(present) = std::env::var("FCAD_TEST_DEPENDENCIES") else {
        return;
    };
    let available = ferritecad_fixtures::native_dependencies(&[
        (present.contains('o'), "OCCT"),
        (present.contains('p'), "PLANEGCS"),
        (present.contains('g'), "GPU"),
    ]);
    assert_eq!(available, present == "opg");
}

#[test]
fn judge_required_dependencies_independently() {
    let executable = std::env::current_exe().expect("the test executable exists");
    for (present, required, passes) in [
        ("opg", "OCCT", true),
        ("og", "OCCT", true),
        ("pg", "PLANEGCS", true),
        ("g", "PLANEGCS", false),
        ("pg", "OCCT", false),
        ("og", "PLANEGCS", false),
        ("op", "GPU", false),
        ("", "", true),
    ] {
        let output = Command::new(&executable)
            .args(["--exact", "dependency_probe", "--nocapture"])
            .env("FCAD_TEST_DEPENDENCIES", present)
            .env("FERRITECAD_REQUIRE_OCCT", "0")
            .env("FERRITECAD_REQUIRE_PLANEGCS", "0")
            .env("FERRITECAD_REQUIRE_GPU", "0")
            .env(format!("FERRITECAD_REQUIRE_{required}"), "1")
            .output()
            .expect("the dependency probe starts");
        assert_eq!(
            output.status.success(),
            passes,
            "present={present}, required={required}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
