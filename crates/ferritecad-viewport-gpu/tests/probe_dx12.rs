// SPDX-License-Identifier: MIT
//
// Temporary: isolates which part of the face target a backend refuses.

#![allow(clippy::panic)]

/// The real renderer, opened five times, reported rather than panicked.
#[test]
fn the_real_renderer_five_times() {
    let mut report = String::new();
    for attempt in 1..=5 {
        match ferritecad_viewport_gpu::Renderer::new() {
            Ok(_) => report.push_str(&format!("{attempt}: ok\n")),
            Err(error) => report.push_str(&format!("{attempt}: {:?} {error}\n", error.kind())),
        }
    }
    panic!("FIVE:\n{report}");
}
