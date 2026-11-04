// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::{DocumentVersion, EditChange};
use ferritecad_jobs::{LiveEvaluator, Snapshot};
use ferritecad_occt::OcctKernel;
use std::sync::mpsc::{self, Sender};

type Reply<T> = Box<dyn FnOnce(Result<T>) + Send>;

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
    Finish(Address, bool),
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
        let command = Command::Edit(address, ticket, expected, changes, cancel, Box::new(reply));
        if let Err(mpsc::SendError(Command::Edit(.., reply))) = self.0.send(command) {
            reply(Err(stopped()));
        }
    }

    pub(crate) fn draw(
        &self,
        address: Address,
        target: Target,
        cancel: CancelToken,
        reply: impl FnOnce(Result<LoadedScene>) + Send + 'static,
    ) {
        let command = Command::Draw(address, target, cancel, Box::new(reply));
        if let Err(mpsc::SendError(Command::Draw(.., reply))) = self.0.send(command) {
            reply(Err(stopped()));
        }
    }

    pub(super) fn finish(&self, address: Address, accept: bool) {
        let _ = self.0.send(Command::Finish(address, accept));
    }
}

fn stopped() -> CadError {
    CadError::kernel("the document's evaluation thread stopped")
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
            if opened.send(Ok((scene, session))).is_err() {
                return;
            }
            let mut pending = None;
            for command in queue {
                match command {
                    Command::Edit(address, ticket, expected, changes, cancel, reply) => {
                        let context = OperationContext::default().with_cancel(cancel);
                        let result = evaluator.prepare_edit(ticket, expected, changes, &context);
                        if result.is_ok() {
                            pending = Some(address);
                        }
                        reply(result);
                    }
                    Command::Draw(address, target, cancel, reply) => {
                        let context = OperationContext::default().with_cancel(cancel);
                        let result = evaluator.prepare_snapshot(target.snapshot(), &context);
                        if result.is_ok() {
                            if matches!(target, Target::Switch(_)) {
                                evaluator.discard();
                            } else {
                                pending = Some(address);
                            }
                        }
                        reply(result);
                    }
                    Command::Finish(address, accept) => {
                        if pending == Some(address) {
                            pending = None;
                            if accept {
                                evaluator.accept();
                            } else {
                                evaluator.discard();
                            }
                        }
                    }
                    Command::Stop => break,
                }
            }
        })
        .map_err(|error| CadError::io("starting document evaluation", error))?;
    let owner = Owner {
        port: Port(commands),
        thread: Some(thread),
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
