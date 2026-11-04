// SPDX-License-Identifier: MIT
//! One kernel and at most two evaluated versions on the document's owning thread.

use ferritecad_document::{Document, DocumentVersion, EditChange};
use ferritecad_eval::LiveModel;
use ferritecad_exchange::Import;
use ferritecad_kernel::{GeometryKernel, OperationContext, TessellationParams};
use ferritecad_scene::{BodyMeshes, LoadedScene, snapshot_from_revision};
use ferritecad_types::{CadError, Result};

use crate::{DocumentSession, EditRequest, Move, ProducedStep, StepCommit};

#[derive(Debug)]
pub struct LiveDocument<K: GeometryKernel, R> {
    session: DocumentSession,
    model: LiveModel,
    kernel: K,
    read_step: R,
    params: TessellationParams,
    picture: Picture,
}

#[derive(Debug)]
struct Picture {
    scene: LoadedScene,
    meshes: BodyMeshes,
}

enum Target<'a> {
    Open,
    Edit(&'a Picture),
    History(&'a Picture),
}

fn evaluate<K: GeometryKernel>(
    document: &Document,
    model: &mut LiveModel,
    kernel: &mut K,
    read_step: impl FnMut(&mut K, &[u8]) -> Result<Import>,
    params: &TessellationParams,
    context: &OperationContext,
    target: Target<'_>,
) -> Result<Picture> {
    let empty = BodyMeshes::default();
    let (accepted, baseline) = match target {
        Target::Open => (&empty, None),
        Target::History(picture) => (&picture.meshes, None),
        Target::Edit(picture) => {
            let facts = picture
                .scene
                .rebuild_facts
                .as_ref()
                .expect("document scene has facts");
            (&picture.meshes, Some(&facts.unbuilt))
        }
    };
    let built = model.prepare(document, kernel, context, baseline)?;
    match snapshot_from_revision(
        document, built, kernel, read_step, params, context, accepted,
    ) {
        Ok((scene, meshes)) => Ok(Picture { scene, meshes }),
        Err(error) => {
            model.discard(kernel);
            Err(error)
        }
    }
}

impl<K, R> LiveDocument<K, R>
where
    K: GeometryKernel,
    R: FnMut(&mut K, &[u8]) -> Result<Import>,
{
    pub fn open(
        session: DocumentSession,
        mut kernel: K,
        mut read_step: R,
        params: TessellationParams,
        context: &OperationContext,
    ) -> Result<Self> {
        let current = session.current();
        let document = Document::open_read_only(current.path())?;
        let mut model = LiveModel::default();
        let picture = evaluate(
            &document,
            &mut model,
            &mut kernel,
            &mut read_step,
            &params,
            context,
            Target::Open,
        )?;
        if let Err(error) = current.remember_rebuild(
            picture
                .scene
                .rebuild_facts
                .as_ref()
                .expect("document scene has rebuild facts"),
        ) {
            model.discard(&mut kernel);
            return Err(error);
        }
        model.accept(&mut kernel);
        Ok(Self {
            session,
            model,
            kernel,
            read_step,
            params,
            picture,
        })
    }

    pub fn scene(&self) -> &LoadedScene {
        &self.picture.scene
    }

    pub fn session(&self) -> &DocumentSession {
        &self.session
    }

    pub fn apply(
        &mut self,
        expected: DocumentVersion,
        changes: Vec<EditChange>,
        context: &OperationContext,
    ) -> Result<PendingRevision<'_, K, R>> {
        let ticket = self.session.begin_step();
        let baseline = self
            .picture
            .scene
            .rebuild_facts
            .as_ref()
            .expect("document scene has facts");
        let mut meshes = None;
        let mut check = |document: &Document, kernel: &mut K, scoped: &OperationContext| {
            let picture = evaluate(
                document,
                &mut self.model,
                kernel,
                &mut self.read_step,
                &self.params,
                scoped,
                Target::Edit(&self.picture),
            )?;
            meshes = Some(picture.meshes);
            Ok(picture.scene)
        };
        let mut run = crate::edit::EditRun {
            baseline: Some(baseline),
            observe: None,
            evaluate: Some(&mut check),
            scene: None,
        };
        let step = ticket.run(|source, _, destination| {
            crate::edit::edit_copy_with(
                &EditRequest {
                    source: source.to_owned(),
                    expected,
                    changes,
                    destination: destination.to_owned(),
                },
                &mut self.kernel,
                context,
                &mut run,
            )
            .map(|_| ())
        });
        let scene = run.scene.take();
        let step = step.inspect_err(|_| self.model.discard(&mut self.kernel))?;
        let scene = scene.expect("prepared candidate scene");
        let picture = Picture {
            scene,
            meshes: meshes.expect("prepared candidate meshes"),
        };
        self.pending(Change::Edit(step), picture, context)
    }

    pub fn undo(&mut self, context: &OperationContext) -> Result<PendingRevision<'_, K, R>> {
        let target = self
            .session
            .begin_undo()
            .ok_or_else(|| CadError::input("nothing to undo"))?;
        self.move_to(target, context)
    }

    pub fn redo(&mut self, context: &OperationContext) -> Result<PendingRevision<'_, K, R>> {
        let target = self
            .session
            .begin_redo()
            .ok_or_else(|| CadError::input("nothing to redo"))?;
        self.move_to(target, context)
    }

    fn move_to(
        &mut self,
        target: Move,
        context: &OperationContext,
    ) -> Result<PendingRevision<'_, K, R>> {
        let document = Document::open_read_only(target.path())?;
        let picture = evaluate(
            &document,
            &mut self.model,
            &mut self.kernel,
            &mut self.read_step,
            &self.params,
            context,
            Target::History(&self.picture),
        )?;
        self.pending(Change::Move(target), picture, context)
    }

    fn pending(
        &mut self,
        change: Change,
        picture: Picture,
        context: &OperationContext,
    ) -> Result<PendingRevision<'_, K, R>> {
        let snapshot = match &change {
            Change::Edit(step) => step.snapshot(),
            Change::Move(target) => target.snapshot(),
        };
        snapshot
            .remember_rebuild(
                picture
                    .scene
                    .rebuild_facts
                    .as_ref()
                    .expect("document scene has facts"),
            )
            .inspect_err(|_| self.model.discard(&mut self.kernel))?;
        Ok(PendingRevision {
            owner: self,
            next: Some((change, picture)),
            cancel: context.cancel().clone(),
        })
    }
}

impl<K: GeometryKernel, R> Drop for LiveDocument<K, R> {
    fn drop(&mut self) {
        self.model.release(&mut self.kernel);
    }
}

#[derive(Debug)]
enum Change {
    Edit(ProducedStep),
    Move(Move),
}

/// Dropping a prepared step cancels it, including its private file and new shapes.
#[derive(Debug)]
pub struct PendingRevision<'a, K: GeometryKernel, R> {
    owner: &'a mut LiveDocument<K, R>,
    next: Option<(Change, Picture)>,
    cancel: ferritecad_kernel::CancelToken,
}

impl<K: GeometryKernel, R> PendingRevision<'_, K, R> {
    pub fn scene(&self) -> &LoadedScene {
        &self.next.as_ref().expect("pending revision").1.scene
    }

    pub fn snapshot(&self) -> &crate::Snapshot {
        match &self.next.as_ref().expect("pending revision").0 {
            Change::Edit(step) => step.snapshot(),
            Change::Move(target) => target.snapshot(),
        }
    }

    pub fn commit_step(mut self) -> Result<StepCommit> {
        self.cancel.check()?;
        let (change, picture) = self.next.take().expect("pending revision");
        let outcome = match change {
            Change::Edit(step) => self.owner.session.commit_step(step)?,
            Change::Move(target) => {
                self.owner.session.commit_move(target)?;
                StepCommit::Accepted
            }
        };
        if outcome == StepCommit::Accepted {
            self.owner.model.accept(&mut self.owner.kernel);
            self.owner.picture = picture;
        }
        Ok(outcome)
    }
}

impl<K: GeometryKernel, R> Drop for PendingRevision<'_, K, R> {
    fn drop(&mut self) {
        self.owner.model.discard(&mut self.owner.kernel);
    }
}

#[cfg(test)]
#[path = "../tests/live_model/suite.rs"]
mod tests;
