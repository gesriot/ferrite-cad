// SPDX-License-Identifier: MIT
use super::*;

pub(crate) fn open(path: &Path) -> Result<OpenTab> {
    open_in(
        &std::env::temp_dir(),
        path,
        ferritecad_jobs::HistoryLimits::default(),
    )
}

pub(crate) fn open_in(
    root: &Path,
    path: &Path,
    limits: ferritecad_jobs::HistoryLimits,
) -> Result<OpenTab> {
    let session = DocumentSession::open_in(root, path, limits)?;
    if ferritecad_occt::is_available() {
        owner::start(&OperationContext::default(), move |_| {
            Ok((session, ferritecad_occt::OcctKernel::new()?))
        })
        .map(|(_, tab)| tab)
    } else {
        observed(session).map(|(_, tab, _)| tab)
    }
}

pub(crate) fn topology(path: &Path, root: &Path) -> Result<OpenTab> {
    let session = DocumentSession::open_in(root, path, Default::default())?;
    let probe = counting::SharedProbe::default();
    let shared = Arc::clone(&probe);
    let (_, mut tab) = owner::start_with(
        &OperationContext::default(),
        move |_| {
            let mut kernel = counting::CountingMock::new();
            kernel.probe = Some(shared);
            Ok((session, kernel))
        },
        |_, _| unreachable!("no imports in this fixture"),
    )?;
    tab.owner.as_mut().expect("owner").probe = Some(probe);
    Ok(tab)
}

#[allow(dead_code)]
#[path = "../../../ferritecad-jobs/tests/support/counting.rs"]
pub(crate) mod counting;

pub(crate) fn observed(
    session: DocumentSession,
) -> Result<(LoadedScene, OpenTab, counting::SharedProbe)> {
    let probe = counting::SharedProbe::default();
    let shared = Arc::clone(&probe);
    let (scene, mut tab) = owner::start_with(
        &OperationContext::default(),
        move |_| {
            let mut kernel = counting::Counting::wrap(ferritecad_kernel::mock::MockKernel::new());
            kernel.probe = Some(shared);
            Ok((session, kernel))
        },
        |_, _| unreachable!("no imports in this fixture"),
    )?;
    tab.owner.as_mut().expect("owner").probe = Some(Arc::clone(&probe));
    Ok((scene, tab, probe))
}

pub(crate) fn probe(sessions: &Sessions) -> counting::SharedProbe {
    Arc::clone(
        sessions
            .owner
            .as_ref()
            .expect("owner")
            .probe
            .as_ref()
            .expect("observed kernel"),
    )
}

pub(crate) fn native_counted(
    root: &Path,
    path: &Path,
) -> Result<(LoadedScene, OpenTab, counting::SharedProbe)> {
    let probe = counting::SharedProbe::default();
    let shared = Arc::clone(&probe);
    let (scene, tab) = open_for_view_with(
        root,
        path,
        &OperationContext::default(),
        move || {
            let mut kernel = counting::Counting::wrap(ferritecad_occt::OcctKernel::new()?);
            kernel.probe = Some(shared);
            Ok(kernel)
        },
        |kernel, bytes| kernel.inner.import_step(bytes),
    )?;
    Ok((scene, tab, probe))
}

pub(crate) fn fail_next(sessions: &Sessions, operation: &'static str) -> usize {
    let probe = probe(sessions);
    let mut state = probe.lock().expect("probe");
    let count = work(state.calls, operation);
    state.fail = Some((operation, count + 1));
    state.live
}

fn work(calls: counting::Calls, operation: &str) -> usize {
    match operation {
        "extrude_count" => calls.extrude_count,
        "revolve_count" => calls.revolve_count,
        "fillet" => calls.fillet,
        "chamfer" => calls.chamfer,
        "tessellate" => calls.tessellate,
        _ => unreachable!("known counted test operation"),
    }
}

pub(crate) fn retry(
    sessions: &mut Sessions,
    request: &ferritecad_jobs::EditRequest,
    operation: &str,
    expected_work: usize,
    accepted_shapes: usize,
) {
    let probe = probe(sessions);
    let before = {
        let mut state = probe.lock().expect("probe");
        assert_eq!(state.live, accepted_shapes);
        state.fail = None;
        state.calls
    };
    let (send, receive) = std::sync::mpsc::channel();
    let address = sessions
        .apply(
            request.expected,
            request.changes.clone(),
            move |_, reply| {
                send.send(reply).expect("reply");
            },
        )
        .expect("same tab works");
    let reply = receive
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("answer");
    assert!(matches!(
        sessions.finish_apply(address, reply),
        Edited::Show(_)
    ));
    let after = probe.lock().expect("probe").calls;
    assert_eq!(
        work(after, operation) - work(before, operation),
        expected_work
    );
    if matches!(operation, "fillet" | "chamfer") {
        assert_eq!(after.extrude_count, before.extrude_count);
    }
    sessions.commit_staged().expect("accept");
    assert!(sessions.finish_scene(address, Ok(())));
}

pub(crate) fn edit(tab: OpenTab, changes: Vec<ferritecad_document::EditChange>) -> OpenTab {
    let mut sessions = Sessions::default();
    sessions.adopt(tab);
    let current = sessions.export_source().expect("open");
    let (send, receive) = std::sync::mpsc::channel();
    let address = sessions
        .apply(current.version(), changes, move |_, reply| {
            send.send(reply).expect("answer");
        })
        .expect("step");
    let reply = receive
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("answer");
    assert!(matches!(
        sessions.finish_apply(address, reply),
        Edited::Show(_)
    ));
    sessions.commit_staged().expect("commit");
    assert!(sessions.finish_scene(address, Ok(())));
    OpenTab {
        session: sessions.session.take().expect("session"),
        owner: sessions.owner.take(),
    }
}
