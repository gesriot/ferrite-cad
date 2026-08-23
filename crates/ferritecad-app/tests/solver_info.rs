// SPDX-License-Identifier: MIT
//! What `ferritecad-viewer --solver-info` answers, asked of the real process.
//!
//! Driven through the built binary rather than the function behind it, because
//! what is being checked is a command: its exit status, what it prints, what it
//! refuses, and above all that it answers at all without opening a window. None
//! of that is visible from inside `main`.
//!
//! # What this command promises
//!
//! - It opens no window, no event loop, no document, no Open CASCADE session
//!   and no GPU surface. A question about which solver a build has must be
//!   answerable on a machine with neither a display nor a graphics device.
//! - Exactly two answers, and the exit code says which: `0` with the provenance
//!   the loaded library gave, or [`EXIT_NO_SOLVER`] with a typed refusal.
//! - Never a skip, and never somebody else's arithmetic. The bench's reference
//!   solver is not a fallback, and a viewer that quietly used one would print
//!   the same shape of answer.
//! - A stray argument beside it is refused, with the same `2` as any other
//!   line this viewer cannot act on.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A build that can solve a sketch said so.
const EXIT_AVAILABLE: i32 = 0;
/// A command line this viewer cannot act on.
const EXIT_USAGE: i32 = 2;
/// A build with no sketch solver in it, answering that and nothing else.
const EXIT_NO_SOLVER: i32 = 3;

/// How long a command that opens nothing is given to answer.
///
/// Generous, because it is not a performance claim: it is the difference
/// between a process that answered and one that is sitting in an event loop
/// waiting for somebody to close a window.
const DEADLINE: Duration = Duration::from_secs(30);

/// The viewer this test was built alongside.
fn viewer() -> PathBuf {
    // `current_exe` is target/<profile>/deps/<test>; the binary is two up.
    let mut path = std::env::current_exe().expect("the test knows where it is");
    path.pop();
    path.pop();
    path.push(format!("ferritecad-viewer{}", std::env::consts::EXE_SUFFIX));
    path
}

/// What the process said and what it exited with.
#[derive(Debug)]
struct Answer {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Answer {
    /// Everything the process said, whichever stream it said it on.
    fn said(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

/// Runs the viewer, and refuses to wait for a window.
///
/// A deadline rather than a plain `output()`: the property being checked is
/// that this command answers and leaves, and a diagnostic that had reached the
/// event loop would sit here for ever rather than fail.
///
/// The display variables are cleared for the same reason. On a machine that has
/// a display that proves nothing; on one that does not — an ordinary Linux CI
/// runner — a path that reached winit cannot start at all, so the difference
/// between answering before the window and answering after it becomes visible.
fn run(arguments: &[&str]) -> Answer {
    let mut child = Command::new(viewer())
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .spawn()
        .expect("the viewer is built alongside this test");

    let deadline = Instant::now() + DEADLINE;
    while child
        .try_wait()
        .expect("a spawned child can be asked whether it has finished")
        .is_none()
    {
        assert!(
            Instant::now() < deadline,
            "ferritecad-viewer {arguments:?} did not answer within {DEADLINE:?}, so it opened \
             something and waited"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    // Both pipes hold a line or two at most, far under the buffer a child can
    // fill, so there is nothing to drain before the wait above.
    let output = child
        .wait_with_output()
        .expect("a finished child hands over what it wrote");
    Answer {
        code: output
            .status
            .code()
            .expect("the viewer exits rather than being signalled"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn the_viewer_says_which_sketch_solver_it_has_and_opens_nothing() {
    let answer = run(&["--solver-info"]);

    // The pairing, in both directions. There are two answers, the exit code
    // has to say which one was given, and each has to look like itself: a
    // build that named a library and exited "no solver", or one that claimed a
    // solver and named nothing, would be describing a state that cannot exist.
    match answer.code {
        EXIT_AVAILABLE => {
            assert!(
                answer.stdout.contains("sketch solver: available"),
                "exit {EXIT_AVAILABLE} without saying a solver is available:\n{}",
                answer.stdout
            );
            let provenance = answer
                .stdout
                .lines()
                .find_map(|line| line.strip_prefix("provenance: "))
                .unwrap_or_else(|| {
                    panic!(
                        "an available solver has to name the library that answered:\n{}",
                        answer.stdout
                    )
                });
            assert!(
                !provenance.trim().is_empty(),
                "the library was named with an empty string"
            );
            assert!(
                !answer.stdout.contains("unavailable"),
                "the same answer said both things:\n{}",
                answer.stdout
            );
        }
        EXIT_NO_SOLVER => {
            assert!(
                answer.stdout.contains("sketch solver: unavailable"),
                "exit {EXIT_NO_SOLVER} without saying there is no solver:\n{}",
                answer.stdout
            );
            assert!(
                !answer.stdout.contains("provenance:"),
                "a build with no solver named a library anyway:\n{}",
                answer.stdout
            );
        }
        other => panic!(
            "--solver-info exited {other}, which is neither of its two answers\nstdout:\n{}\n\
             stderr:\n{}",
            answer.stdout, answer.stderr
        ),
    }

    // Never a skip, and never the bench. The reference Levenberg-Marquardt
    // that the solver bench keeps is not a fallback for a missing planegcs: a
    // viewer that quietly used one would print an answer of exactly this
    // shape, so what it must never do is named here rather than assumed.
    let said = answer.said();
    for forbidden in [
        "skip",
        "solver-lab",
        "solver_lab",
        "Levenberg",
        "levenberg",
        "reference",
    ] {
        assert!(
            !said.contains(forbidden),
            "--solver-info said {forbidden:?}, and it may not:\n{said}"
        );
    }
}

/// A build that cannot link planegcs answers that, in the solver crate's own
/// typed words.
///
/// Keyed on the cargo feature rather than on what came back. Without the
/// feature the build script cannot link a library at all, so this build has no
/// solver and that is certain here. With the feature, whether a library was
/// actually found is a question about the machine, and the pin workflow is
/// where it is answered against a real one.
#[cfg(not(feature = "planegcs"))]
#[test]
fn a_build_without_planegcs_answers_a_typed_unavailable() {
    let answer = run(&["--solver-info"]);
    assert_eq!(
        answer.code, EXIT_NO_SOLVER,
        "a build that cannot link planegcs did not say so\nstdout:\n{}\nstderr:\n{}",
        answer.stdout, answer.stderr
    );
    assert!(
        answer.stdout.contains("sketch solver: unavailable"),
        "{}",
        answer.stdout
    );
    // The typed refusal's own sentence, forwarded rather than paraphrased. A
    // viewer that wrote its own would be free to keep saying it after the
    // solver crate had changed its mind.
    assert!(
        answer.stdout.contains("did not link planegcs"),
        "the refusal was not the solver crate's own:\n{}",
        answer.stdout
    );
    assert!(
        answer.stdout.contains("tools/build-planegcs.sh"),
        "the refusal does not say what to do about it:\n{}",
        answer.stdout
    );
}

/// A build that was required to link planegcs answers with the library it
/// loaded.
///
/// Keyed on `FERRITECAD_REQUIRE_PLANEGCS`, which is the variable that makes the
/// solver crate's build script refuse a build with no library. Under it, "this
/// build has no sketch solver" is not an answer that can honestly come back,
/// and the pin workflow is the run that sets it.
///
/// Without the variable there is nothing here to check rather than something
/// being let off: the feature can be on while no library was found, which is
/// what an ordinary `--all-features` build on a machine that has never built
/// planegcs looks like, and `Unavailable` is then the correct answer.
#[cfg(feature = "planegcs")]
#[test]
fn a_required_build_answers_with_the_library_it_loaded() {
    if std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref() != Ok("1") {
        return;
    }
    let answer = run(&["--solver-info"]);
    assert_eq!(
        answer.code, EXIT_AVAILABLE,
        "a build that had to link planegcs did not answer that it has a solver\nstdout:\n{}\n\
         stderr:\n{}",
        answer.stdout, answer.stderr
    );
    assert!(
        answer.stdout.contains("sketch solver: available"),
        "{}",
        answer.stdout
    );
    assert!(
        answer
            .stdout
            .lines()
            .any(|line| line.starts_with("provenance: ")),
        "the application claimed a solver and named no library:\n{}",
        answer.stdout
    );
}

#[test]
fn a_stray_argument_beside_the_diagnostic_is_refused() {
    for line in [
        vec!["--solver-info", "part.fcad"],
        vec!["--solver-info", "--solver-info"],
        vec!["--solver-info", ""],
    ] {
        let answer = run(&line);
        assert_eq!(
            answer.code, EXIT_USAGE,
            "{line:?} was accepted\nstdout:\n{}\nstderr:\n{}",
            answer.stdout, answer.stderr
        );
        assert!(
            answer.stderr.contains("usage"),
            "{line:?} was refused without saying how to use it: {}",
            answer.stderr
        );
        assert!(
            !answer.said().contains("sketch solver"),
            "{line:?} was refused and answered anyway:\n{}",
            answer.said()
        );
    }
}

/// A document argument still goes to the window, and not to the diagnostic.
///
/// Only where a window cannot open. With no display the viewer gets as far as
/// asking for one and fails there, which is the evidence wanted: it took the
/// window path rather than the usage path or the diagnostic one. On a machine
/// with a display it would open a window and wait, which is not a thing a test
/// can assert against, so this is compiled where the condition can be arranged
/// rather than skipped where it cannot.
#[cfg(target_os = "linux")]
#[test]
fn a_document_argument_still_reaches_the_window() {
    let answer = run(&["part.fcad"]);
    assert_ne!(
        answer.code, EXIT_USAGE,
        "a document was refused as a usage error\nstderr:\n{}",
        answer.stderr
    );
    assert_ne!(
        answer.code, EXIT_NO_SOLVER,
        "a document was answered as a question about the solver\nstdout:\n{}",
        answer.stdout
    );
    assert!(
        !answer.said().contains("sketch solver"),
        "opening a document answered a question nobody asked:\n{}",
        answer.said()
    );
}
