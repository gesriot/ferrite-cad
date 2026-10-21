// SPDX-License-Identifier: MIT
use crate::{App, AppEvent, sessions};
use std::path::PathBuf;

impl App {
    /// Stages the picture of a version that is waiting to become current.
    pub(crate) fn stage_scene(&mut self, generation: sessions::Address, path: PathBuf) {
        let Some(cancel) = self.sessions.scene_token(generation) else {
            return;
        };
        let proxy = self.proxy.clone();
        if self.sessions.staged_keeps_picture(generation) {
            let worker = sessions::spawn_kept_facts(path, cancel, move |result| {
                let _ = proxy.send_event(AppEvent::KeptFacts {
                    generation,
                    result: Box::new(result),
                });
            });
            self.sessions.attach_scene(generation, worker);
        } else {
            self.sessions.stage_picture(
                generation,
                path,
                move |result| {
                    let _ = proxy.send_event(AppEvent::SceneStaged {
                        generation,
                        result: Box::new(result),
                    });
                },
                sessions::spawn_scene,
            );
        }
    }
}
