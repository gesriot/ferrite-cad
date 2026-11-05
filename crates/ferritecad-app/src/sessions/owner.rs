// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::{DocumentVersion, EditChange};
use ferritecad_jobs::{LiveEvaluator, Snapshot};
use ferritecad_occt::OcctKernel;
use std::sync::mpsc::{self, Sender};

struct Reply<T>(Option<Box<dyn FnOnce(Result<T>) + Send>>);

impl<T> Reply<T> {
    fn new(reply: impl FnOnce(Result<T>) + Send + 'static) -> Self {
        Self(Some(Box::new(reply)))
    }

    fn send(mut self, result: Result<T>) {
        self.0.take().expect("one reply")(result);
    }
}

impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        if let Some(reply) = self.0.take() {
            reply(Err(stopped()));
        }
    }
}

enum Command {
    Edit(
        Address,
        StepTicket,
        DocumentVersion,
        Vec<EditChange>,
        CancelToken,
        Reply<ProducedStep>,
    ),
    Draw(Address, Target, CancelToken, Reply<LoadedScene>),
    Finish(Address, bool, Sender<()>),
    Stop,
}

pub(crate) enum Target {
    History(Move),
    Restore(Arc<Snapshot>),
    Switch(Arc<Snapshot>),
}

impl Target {
    fn snapshot(&self) -> &Snapshot {
        match self {
            Self::History(target) => target.snapshot(),
            Self::Restore(target) | Self::Switch(target) => target,
        }
    }
}

#[derive(Debug)]
pub(crate) struct OpenTab {
    pub(super) session: DocumentSession,
    pub(super) owner: Option<Owner>,
}

impl std::ops::Deref for OpenTab {
    type Target = DocumentSession;

    fn deref(&self) -> &DocumentSession {
        &self.session
    }
}

#[cfg(test)]
impl From<DocumentSession> for OpenTab {
    fn from(session: DocumentSession) -> Self {
        Self {
            session,
            owner: None,
        }
    }
}

impl std::ops::DerefMut for OpenTab {
    fn deref_mut(&mut self) -> &mut DocumentSession {
        &mut self.session
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Port(Sender<Command>);

#[derive(Debug)]
pub(super) struct Owner {
    pub(super) port: Port,
    thread: Option<JoinHandle<()>>,
    #[cfg(test)]
    pub(crate) probe: Option<super::test_open::counting::SharedProbe>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.port.0.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Port {
    pub(super) fn edit(
        &self,
        address: Address,
        ticket: StepTicket,
        expected: DocumentVersion,
        changes: Vec<EditChange>,
        cancel: CancelToken,
        reply: impl FnOnce(Result<ProducedStep>) + Send + 'static,
    ) {
        let command = Command::Edit(
            address,
            ticket,
            expected,
            changes,
            cancel,
            Reply::new(reply),
        );
        if let Err(mpsc::SendError(Command::Edit(.., reply))) = self.0.send(command) {
            reply.send(Err(stopped()));
        }
    }

    pub(crate) fn draw(
        &self,
        address: Address,
        target: Target,
        cancel: CancelToken,
        reply: impl FnOnce(Result<LoadedScene>) + Send + 'static,
    ) {
        let command = Command::Draw(address, target, cancel, Reply::new(reply));
        if let Err(mpsc::SendError(Command::Draw(.., reply))) = self.0.send(command) {
            reply.send(Err(stopped()));
        }
    }

    pub(super) fn finish(&self, address: Address, accept: bool) {
        let (acknowledge, finished) = mpsc::channel();
        if self
            .0
            .send(Command::Finish(address, accept, acknowledge))
            .is_ok()
        {
            let _ = finished.recv();
        }
    }
}

fn stopped() -> CadError {
    CadError::kernel(
        "the document's evaluation thread stopped. \
         Please open the document again to continue editing",
    )
}

pub(super) fn start(
    context: &OperationContext,
    open: impl FnOnce(&OperationContext) -> Result<(DocumentSession, OcctKernel)> + Send + 'static,
) -> Result<(LoadedScene, OpenTab)> {
    start_with(context, open, OcctKernel::import_step)
}

pub(super) fn start_with<K, R>(
    context: &OperationContext,
    open: impl FnOnce(&OperationContext) -> Result<(DocumentSession, K)> + Send + 'static,
    read_step: R,
) -> Result<(LoadedScene, OpenTab)>
where
    K: ferritecad_kernel::GeometryKernel + 'static,
    R: FnMut(&mut K, &[u8]) -> Result<ferritecad_exchange::Import> + Send + 'static,
{
    let context = context.clone();
    let (commands, queue) = mpsc::channel();
    let (opened, initial) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("document evaluation".into())
        .spawn(move || {
            let result = open(&context).and_then(|(session, kernel)| {
                let (evaluator, scene) = LiveEvaluator::open(
                    &session.current(),
                    kernel,
                    read_step,
                    Default::default(),
                    &context,
                )?;
                Ok((session, scene, evaluator))
            });
            let (session, scene, mut evaluator) = match result {
                Ok(ready) => ready,
                Err(error) => {
                    let _ = opened.send(Err(error));
                    return;
                }
            };
            let mut accepted_scene = scene.clone();
            let mut candidate_scene = None;
            if opened.send(Ok((scene, session))).is_err() {
                return;
            }
            let mut pending = None;
            while let Ok(command) = queue.recv() {
                match command {
                    Command::Edit(address, ticket, expected, changes, cancel, reply) => {
                        let context = OperationContext::default().with_cancel(cancel);
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            evaluator.prepare_edit(ticket, expected, changes, &context)
                        }));
                        let Ok(result) = result else {
                            drop(queue);
                            reply.send(Err(stopped()));
                            return;
                        };
                        if let Ok(step) = &result {
                            candidate_scene = step.scene().cloned();
                            pending = Some(address);
                        }
                        reply.send(result);
                    }
                    Command::Draw(address, target, cancel, reply) => {
                        let context = OperationContext::default().with_cancel(cancel);
                        let result =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                || match &target {
                                    Target::Switch(snapshot) => {
                                        context.check_cancelled()?;
                                        let document =
                                            ferritecad_document::Document::open_read_only(
                                                snapshot.path(),
                                            )?;
                                        if document.model_without_checkpoints()?
                                            != accepted_scene
                                                .rebuild_facts
                                                .as_ref()
                                                .expect("accepted facts")
                                                .model
                                        {
                                            return Err(CadError::input(
                                                "the tab's accepted model changed",
                                            ));
                                        }
                                        let mut scene = accepted_scene.clone();
                                        if let Some(source) = &mut scene.edit_source {
                                            source.version = snapshot.version();
                                        }
                                        Ok(scene)
                                    }
                                    Target::History(_) | Target::Restore(_) => {
                                        evaluator.prepare_snapshot(target.snapshot(), &context)
                                    }
                                },
                            ));
                        let Ok(result) = result else {
                            drop(queue);
                            reply.send(Err(stopped()));
                            return;
                        };
                        if let Ok(scene) = &result
                            && !matches!(target, Target::Switch(_))
                        {
                            candidate_scene = Some(scene.clone());
                            pending = Some(address);
                        }
                        // Delivery may immediately inspect or release the snapshot's other leases.
                        drop(target);
                        reply.send(result);
                    }
                    Command::Finish(address, accept, acknowledge) => {
                        if pending == Some(address) {
                            pending = None;
                            if accept {
                                evaluator.accept();
                                accepted_scene = candidate_scene.take().expect("prepared picture");
                            } else {
                                evaluator.discard();
                                candidate_scene = None;
                            }
                        }
                        let _ = acknowledge.send(());
                    }
                    Command::Stop => break,
                }
            }
        })
        .map_err(|error| CadError::io("starting document evaluation", error))?;
    let owner = Owner {
        port: Port(commands),
        thread: Some(thread),
        #[cfg(test)]
        probe: None,
    };
    let (scene, session) = initial.recv().map_err(|_| stopped())??;
    Ok((
        scene,
        OpenTab {
            session,
            owner: Some(owner),
        },
    ))
}
