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
    evaluator: LiveEvaluator<K, R>,
    scene: LoadedScene,
}

/// Kernel state confined to one owner. Only the ticket, scene and facts cross threads.
#[derive(Debug)]
pub struct LiveEvaluator<K: GeometryKernel, R> {
    model: LiveModel,
    kernel: K,
    read_step: R,
    params: TessellationParams,
    accepted: BodyMeshes,
    candidate: Option<BodyMeshes>,
}

#[allow(
    clippy::too_many_arguments,
    reason = "the edit transaction borrows the kernel separately from the evaluator state"
)]
fn evaluate<K: GeometryKernel>(
    document: &Document,
    model: &mut LiveModel,
    kernel: &mut K,
    read_step: impl FnMut(&mut K, &[u8]) -> Result<Import>,
    params: &TessellationParams,
    context: &OperationContext,
    accepted: &BodyMeshes,
    baseline: Option<&ferritecad_scene::RebuildFacts>,
) -> Result<(LoadedScene, BodyMeshes)> {
    let baseline = baseline.map(|facts| &facts.unbuilt);
    let built = model
        .prepare(document, kernel, context, baseline)
        .map_err(|error| crate::edit::describe_rebuild_refusal(document, error))?;
    match snapshot_from_revision(
        document, built, kernel, read_step, params, context, accepted,
    ) {
        Ok(picture) => Ok(picture),
        Err(error) => {
            model.discard(kernel);
            Err(error)
        }
    }
}

impl<K, R> LiveEvaluator<K, R>
where
    K: GeometryKernel,
    R: FnMut(&mut K, &[u8]) -> Result<Import>,
{
    pub fn open(
        current: &crate::Snapshot,
        kernel: K,
        read_step: R,
        params: TessellationParams,
        context: &OperationContext,
    ) -> Result<(Self, LoadedScene)> {
        let mut owner = Self {
            kernel,
            read_step,
            params,
            model: LiveModel::default(),
            accepted: BodyMeshes::default(),
            candidate: None,
        };
        let scene = owner.prepare_snapshot(current, context)?;
        owner.accept();
        Ok((owner, scene))
    }

    pub fn prepare_edit(
        &mut self,
        ticket: crate::StepTicket,
        expected: DocumentVersion,
        changes: Vec<EditChange>,
        context: &OperationContext,
    ) -> Result<ProducedStep> {
        if self.candidate.is_some() {
            return Err(CadError::input(
                "finish the pending revision before preparing another",
            ));
        }
        ticket.check_form_version(expected)?;
        context.check_cancelled()?;
        let mut meshes = None;
        let mut scene = None;
        let step = ticket.run_from_snapshot(|source, destination| {
            let baseline = source.rebuild_facts().ok_or_else(|| {
                CadError::input("load the session picture before editing its model")
            })?;
            let mut check = |document: &Document, kernel: &mut K, scoped: &OperationContext| {
                let picture = evaluate(
                    document,
                    &mut self.model,
                    kernel,
                    &mut self.read_step,
                    &self.params,
                    scoped,
                    &self.accepted,
                    Some(baseline),
                )?;
                meshes = Some(picture.1);
                Ok(picture.0)
            };
            let mut run = crate::edit::EditRun {
                baseline: Some(baseline),
                evaluate: Some(&mut check),
                scene: None,
            };
            let result = crate::edit::edit_copy_with(
                &EditRequest {
                    source: source.path().to_owned(),
                    expected,
                    changes,
                    destination: destination.to_owned(),
                },
                &mut self.kernel,
                context,
                &mut run,
            );
            scene = run.scene.take();
            result.map(|_| ())
        });
        let mut step = step.inspect_err(|_| self.discard())?;
        let scene = scene.expect("prepared candidate scene");
        self.stage(step.snapshot(), &scene, meshes.expect("prepared meshes"))?;
        step.scene = Some(Box::new(scene));
        Ok(step)
    }

    pub fn prepare_snapshot(
        &mut self,
        target: &crate::Snapshot,
        context: &OperationContext,
    ) -> Result<LoadedScene> {
        let document = Document::open_read_only(target.path())?;
        let (scene, meshes) = evaluate(
            &document,
            &mut self.model,
            &mut self.kernel,
            &mut self.read_step,
            &self.params,
            context,
            &self.accepted,
            None,
        )?;
        self.stage(target, &scene, meshes)?;
        Ok(scene)
    }

    fn stage(
        &mut self,
        target: &crate::Snapshot,
        scene: &LoadedScene,
        meshes: BodyMeshes,
    ) -> Result<()> {
        let facts = scene
            .rebuild_facts
            .as_ref()
            .expect("document scene has facts");
        target
            .remember_rebuild(facts)
            .inspect_err(|_| self.discard())?;
        self.candidate = Some(meshes);
        Ok(())
    }
}

impl<K: GeometryKernel, R> LiveEvaluator<K, R> {
    pub fn accept(&mut self) {
        self.accepted = self.candidate.take().expect("prepared meshes");
        self.model.accept(&mut self.kernel);
    }

    pub fn discard(&mut self) {
        self.candidate = None;
        self.model.discard(&mut self.kernel);
    }
}

impl<K: GeometryKernel, R> Drop for LiveEvaluator<K, R> {
    fn drop(&mut self) {
        self.model.release(&mut self.kernel);
    }
}

impl<K, R> LiveDocument<K, R>
where
    K: GeometryKernel,
    R: FnMut(&mut K, &[u8]) -> Result<Import>,
{
    pub fn open(
        session: DocumentSession,
        kernel: K,
        read_step: R,
        params: TessellationParams,
        context: &OperationContext,
    ) -> Result<Self> {
        let (evaluator, scene) =
            LiveEvaluator::open(&session.current(), kernel, read_step, params, context)?;
        Ok(Self {
            session,
            evaluator,
            scene,
        })
    }

    pub fn scene(&self) -> &LoadedScene {
        &self.scene
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
        let mut step = self
            .evaluator
            .prepare_edit(ticket, expected, changes, context)?;
        let scene = step.take_scene().expect("prepared candidate scene");
        Ok(self.pending(Change::Edit(step), scene, context))
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
        let scene = self
            .evaluator
            .prepare_snapshot(target.snapshot(), context)?;
        Ok(self.pending(Change::Move(target), scene, context))
    }

    fn pending(
        &mut self,
        change: Change,
        scene: LoadedScene,
        context: &OperationContext,
    ) -> PendingRevision<'_, K, R> {
        PendingRevision {
            owner: self,
            next: Some((change, scene)),
            cancel: context.cancel().clone(),
        }
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
    next: Option<(Change, LoadedScene)>,
    cancel: ferritecad_kernel::CancelToken,
}

impl<K: GeometryKernel, R> PendingRevision<'_, K, R> {
    pub fn scene(&self) -> &LoadedScene {
        &self.next.as_ref().expect("pending revision").1
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
            self.owner.evaluator.accept();
            self.owner.scene = picture;
        }
        Ok(outcome)
    }
}

impl<K: GeometryKernel, R> Drop for PendingRevision<'_, K, R> {
    fn drop(&mut self) {
        self.owner.evaluator.discard();
    }
}

#[cfg(test)]
#[path = "../tests/live_model/suite.rs"]
mod tests;
