// SPDX-License-Identifier: MIT
//
// Temporary: isolates which part of the face target a backend refuses.

#![allow(clippy::panic)]

/// The real renderer, opened one after another and all kept alive.
///
/// The smoke suite opens one device per test and runs them in parallel, so
/// what matters is not whether one renderer can be built but whether the
/// twentieth can while the first nineteen are still there.
#[test]
fn many_renderers_at_once() {
    let mut report = String::new();
    let mut held = Vec::new();
    for attempt in 1..=24 {
        match ferritecad_viewport_gpu::Renderer::new() {
            Ok(renderer) => {
                held.push(renderer);
                report.push_str(&format!("{attempt}: ok\n"));
            }
            Err(error) => {
                report.push_str(&format!("{attempt}: {:?} {error}\n", error.kind()));
                break;
            }
        }
    }
    panic!("HELD:\n{report}");
}
