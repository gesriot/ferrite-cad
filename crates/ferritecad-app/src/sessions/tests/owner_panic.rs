// SPDX-License-Identifier: MIT
use super::*;
use std::time::Duration;

#[test]
fn a_kernel_panic_answers_and_stops_the_owner() {
    let f = fixture();
    let source_bytes = std::fs::read(&f.file).expect("source");
    let session = DocumentSession::open(&f.file).expect("session");
    let (_, tab, probe) = test_open::observed(session).expect("opened");
    let original = tab.current();
    let original_bytes = std::fs::read(original.path()).expect("private version");
    let mut sessions = Sessions::default();
    sessions.adopt(tab);
    let before = private_files(&sessions);
    {
        let mut probe = probe.lock().expect("probe");
        probe.panic = Some(("extrude_count", probe.calls.extrude_count + 1));
    }
    let (send, receive) = mpsc::channel();
    let address = sessions
        .apply(
            original.version(),
            vec![
                ferritecad_document::EditChange::constant(
                    f.feature,
                    ferritecad_document::ExpressionSlot::ExtrudeDistance,
                    30.,
                )
                .expect("value"),
            ],
            move |_, reply| {
                let _ = send.send(reply);
            },
        )
        .expect("started");
    let reply = receive
        .recv_timeout(Duration::from_secs(3))
        .expect("panic must answer");
    assert_eq!(
        reply.as_ref().expect_err("kernel panic").kind(),
        ErrorKind::Kernel
    );
    assert_eq!(sessions.finish_apply(address, reply), Edited::Failed);
    assert!(!sessions.busy() && !sessions.dirty() && !sessions.can_undo());
    assert_eq!(
        sessions.export_source().expect("open").version(),
        original.version()
    );
    assert_eq!(private_files(&sessions), before);
    assert_eq!(std::fs::read(&f.file).expect("file"), source_bytes);
    assert_eq!(
        std::fs::read(original.path()).expect("copy"),
        original_bytes
    );
    assert!(sessions.status.contains("open the document again"));
    let (send, receive) = mpsc::channel();
    let address = sessions
        .apply(
            original.version(),
            vec![
                ferritecad_document::EditChange::constant(
                    f.feature,
                    ferritecad_document::ExpressionSlot::ExtrudeDistance,
                    31.,
                )
                .expect("value"),
            ],
            move |_, reply| {
                let _ = send.send(reply);
            },
        )
        .expect("request rejected through the ordinary reply");
    let reply = receive
        .recv_timeout(Duration::from_secs(3))
        .expect("closed owner answers");
    assert_eq!(
        reply.as_ref().expect_err("stopped").kind(),
        ErrorKind::Kernel
    );
    assert_eq!(sessions.finish_apply(address, reply), Edited::Failed);
    assert!(!sessions.can_save() && sessions.can_save_as());
    sessions.stop_all();
    assert!(probe.lock().expect("probe").closed);
}

#[test]
fn a_panicked_owner_refuses_history_but_preserves_saving() {
    let f = fixture();
    let (_, tab, probe) =
        test_open::observed(DocumentSession::open(&f.file).expect("session")).expect("opened");
    let mut sessions = Sessions::default();
    sessions.adopt(tab);
    accept(&mut sessions, f.feature, 20.);
    accept(&mut sessions, f.feature, 22.);
    let address = sessions.begin_move(true).expect("undo").0;
    let (send, receive) = mpsc::channel();
    sessions.draw_staged(address, move |result| {
        send.send(result).expect("answer");
    });
    receive
        .recv_timeout(Duration::from_secs(3))
        .expect("reply")
        .expect("picture");
    sessions.commit_staged().expect("undo");
    assert!(sessions.finish_scene(address, Ok(())));
    let accepted = sessions.export_source().expect("accepted");
    let files = private_files(&sessions);
    {
        let mut state = probe.lock().expect("probe");
        state.panic = Some(("extrude_count", state.calls.extrude_count + 1));
    }
    let (address, edited) = apply(&mut sessions, f.feature, 30.);
    assert_eq!(edited, Edited::Failed);
    assert_eq!(
        sessions.export_source().expect("accepted").version(),
        accepted.version()
    );
    assert_eq!(private_files(&sessions), files);
    assert!(sessions.dirty() && sessions.can_undo() && sessions.can_redo());
    for undo in [true, false] {
        let address = sessions.begin_move(undo).expect("history remains").0;
        let (send, receive) = mpsc::channel();
        sessions.draw_staged(address, move |result| {
            send.send(result).expect("reply");
        });
        let reply = receive
            .recv_timeout(Duration::from_secs(3))
            .expect("stopped owner answers");
        assert_eq!(
            reply.as_ref().expect_err("stopped").kind(),
            ErrorKind::Kernel
        );
        assert!(sessions.finish_scene(address, reply.map(|_| ())));
        assert_eq!(
            sessions.export_source().expect("same").version(),
            accepted.version()
        );
    }
    assert!(sessions.can_save() && sessions.can_save_as());
    assert!(
        save(&mut sessions, SaveTarget::InPlace, None)
            .expect("save")
            .published
    );
    assert_eq!(height_of(&f.file), 20.);
    let copy = f.root.path().join("after-panic.fcad");
    assert!(
        save(&mut sessions, SaveTarget::As(copy.clone()), None)
            .expect("Save As")
            .published
    );
    assert_eq!(height_of(&copy), 20.);
    assert_eq!(
        sessions.finish_apply(address, Err(CadError::kernel("late"))),
        Edited::Ignore
    );
    sessions.stop_all();
    assert!(probe.lock().expect("probe").closed);
}

#[test]
fn two_tab_owners_release_only_their_own_revisions() {
    let f = fixture();
    let (_, a, a_probe) =
        test_open::observed(DocumentSession::open(&f.file).expect("A")).expect("A opens");
    let (_, b, b_probe) =
        test_open::observed(DocumentSession::open(&f.file).expect("B")).expect("B opens");
    let mut first = Sessions::default();
    let mut second = Sessions::default();
    first.adopt(a);
    second.adopt(b);
    let untouched = b_probe.lock().expect("probe").calls;
    accept(&mut first, f.feature, 25.);
    assert_eq!(b_probe.lock().expect("probe").calls, untouched);
    assert_eq!(a_probe.lock().expect("probe").live, 1);
    first.stop_all();
    assert!(a_probe.lock().expect("probe").closed);
    assert_eq!(a_probe.lock().expect("probe").live, 0);
    assert_eq!(b_probe.lock().expect("probe").live, 1);
    accept(&mut second, f.feature, 20.);
    assert_eq!(
        b_probe.lock().expect("probe").calls.extrude_count - untouched.extrude_count,
        1
    );
    second.stop_all();
    assert!(b_probe.lock().expect("probe").closed);
    assert_eq!(b_probe.lock().expect("probe").live, 0);
}

#[test]
fn a_stale_reply_cannot_discard_another_pending_revision() {
    let f = fixture();
    let (_, tab, probe) =
        test_open::observed(DocumentSession::open(&f.file).expect("session")).expect("opens");
    let port = tab.owner.as_ref().expect("owner").port.clone();
    let mut sessions = Sessions::default();
    sessions.adopt(tab);
    let current = sessions.export_source().expect("open");
    let (send, receive) = mpsc::channel();
    let address = sessions
        .apply(
            current.version(),
            vec![
                ferritecad_document::EditChange::constant(
                    f.feature,
                    ferritecad_document::ExpressionSlot::ExtrudeDistance,
                    30.,
                )
                .expect("height"),
            ],
            move |_, reply| {
                send.send(reply).expect("reply");
            },
        )
        .expect("start");
    let reply = receive
        .recv_timeout(Duration::from_secs(3))
        .expect("answer");
    assert_eq!(probe.lock().expect("probe").live, 2);
    port.finish(address + 1, false);
    assert_eq!(probe.lock().expect("probe").live, 2);
    assert_eq!(
        sessions.finish_apply(address + 1, Err(CadError::input("stale"))),
        Edited::Ignore
    );
    assert!(!sessions.finish_scene(address + 1, Ok(())));
    assert_eq!(probe.lock().expect("probe").live, 2);
    assert!(matches!(
        sessions.finish_apply(address, reply),
        Edited::Show(_)
    ));
    assert!(sessions.finish_scene(address, Err(CadError::rendering("upload refusal"))));
    assert_eq!(probe.lock().expect("probe").live, 1);
    assert_eq!(
        sessions.export_source().expect("accepted").version(),
        current.version()
    );
    assert!(!sessions.dirty() && !sessions.can_undo());
    accept(&mut sessions, f.feature, 30.);
    let calls = probe.lock().expect("probe").calls;
    assert_eq!(
        calls.extrude_count, 3,
        "one open, one rejected candidate, one accepted candidate"
    );
    sessions.stop_all();
    let state = probe.lock().expect("probe");
    assert_eq!(state.live, 0);
    assert_eq!(state.calls.released, 3);
    drop(state);
    assert_eq!(
        sessions.finish_apply(address, Err(CadError::input("closed"))),
        Edited::Ignore
    );
    assert_eq!(probe.lock().expect("probe").calls.released, 3);
}

#[test]
fn a_second_owner_request_cannot_discard_the_first_candidate() {
    let f = fixture();
    let (_, tab, probe) =
        test_open::observed(DocumentSession::open(&f.file).expect("session")).expect("open");
    let port = tab.owner.as_ref().expect("owner").port.clone();
    let mut sessions = Sessions::default();
    sessions.adopt(tab);
    let current = sessions.export_source().expect("open");
    let changes = || {
        vec![
            ferritecad_document::EditChange::constant(
                f.feature,
                ferritecad_document::ExpressionSlot::ExtrudeDistance,
                30.,
            )
            .expect("height"),
        ]
    };
    let (send, receive) = mpsc::channel();
    let address = sessions
        .apply(current.version(), changes(), move |_, reply| {
            send.send(reply).expect("answer");
        })
        .expect("first");
    let first = receive
        .recv_timeout(Duration::from_secs(3))
        .expect("answer")
        .expect("candidate");
    assert_eq!(probe.lock().expect("probe").live, 2);
    let ticket = sessions.session.as_mut().expect("session").begin_step();
    let (send, receive) = mpsc::channel();
    port.edit(
        address + 1,
        ticket,
        current.version(),
        changes(),
        CancelToken::new(),
        move |reply| {
            send.send(reply).expect("answer");
        },
    );
    let reply = receive
        .recv_timeout(Duration::from_secs(3))
        .expect("answer");
    assert_eq!(reply.expect_err("one candidate").kind(), ErrorKind::Input);
    assert_eq!(probe.lock().expect("probe").live, 2);
    assert_eq!(probe.lock().expect("probe").calls.extrude_count, 2);
    assert!(matches!(
        sessions.finish_apply(address, Ok(first)),
        Edited::Show(_)
    ));
    sessions.commit_staged().expect("first remains usable");
    assert!(sessions.finish_scene(address, Ok(())));
    assert_eq!(probe.lock().expect("probe").live, 1);
}

#[test]
fn cancellation_discards_only_candidate_shapes_and_keeps_the_owner_reusable() {
    let f = fixture();
    let (_, tab, probe) =
        test_open::observed(DocumentSession::open(&f.file).expect("session")).expect("open");
    let mut sessions = Sessions::default();
    sessions.adopt(tab);
    let accepted = sessions.export_source().expect("accepted");
    let files = private_files(&sessions);
    let bytes = std::fs::read(accepted.path()).expect("accepted bytes");
    let (send, receive) = mpsc::channel();
    let address = sessions
        .apply(
            accepted.version(),
            vec![
                ferritecad_document::EditChange::constant(
                    f.feature,
                    ferritecad_document::ExpressionSlot::ExtrudeDistance,
                    30.,
                )
                .expect("height"),
            ],
            move |_, reply| {
                send.send(reply).expect("reply");
            },
        )
        .expect("start");
    let reply = receive
        .recv_timeout(Duration::from_secs(30))
        .expect("answer");
    assert_eq!(probe.lock().expect("probe").live, 2);
    assert!(sessions.cancel());
    assert_eq!(sessions.finish_apply(address, reply), Edited::Failed);
    assert_eq!(probe.lock().expect("probe").live, 1);
    assert_eq!(private_files(&sessions), files);
    assert_eq!(std::fs::read(accepted.path()).expect("same bytes"), bytes);
    assert_eq!(
        sessions.export_source().expect("accepted").version(),
        accepted.version()
    );
    assert!(!sessions.busy() && !sessions.dirty() && !sessions.can_undo());
    let before = probe.lock().expect("probe").calls;
    accept(&mut sessions, f.feature, 30.);
    let state = probe.lock().expect("probe");
    assert_eq!(state.calls.extrude_count - before.extrude_count, 1);
    assert_eq!(state.calls.tessellate - before.tessellate, 1);
    assert_eq!(state.live, 1);
}

#[test]
fn a_switch_reply_releases_its_input_snapshot_before_delivery() {
    let f = fixture();
    let (_, tab, probe) =
        test_open::observed(DocumentSession::open(&f.file).expect("session")).expect("open");
    let mut sessions = Sessions::default();
    sessions.adopt(tab);
    let snapshot = sessions.export_source().expect("accepted");
    let leases = Arc::strong_count(&snapshot);
    let weak = Arc::downgrade(&snapshot);
    let before = probe.lock().expect("probe").calls;
    let (send, receive) = mpsc::channel();
    sessions.owner.as_ref().expect("owner").port.draw(
        sessions.address(1),
        owner::Target::Switch(Arc::clone(&snapshot)),
        CancelToken::new(),
        move |reply| {
            send.send((reply, weak.strong_count())).expect("answer");
        },
    );
    let (reply, delivered_leases) = receive
        .recv_timeout(Duration::from_secs(30))
        .expect("answer");
    reply.expect("accepted picture");
    assert_eq!(
        delivered_leases, leases,
        "the request has released its lease"
    );
    assert_eq!(probe.lock().expect("probe").calls, before);
}

#[test]
fn switching_checks_the_accepted_catalogue_without_touching_the_kernel() {
    let f = fixture();
    let (_, tab, probe) =
        test_open::observed(DocumentSession::open(&f.file).expect("session")).expect("open");
    let port = tab.owner.as_ref().expect("owner").port.clone();
    let snapshot = tab.current();
    let before = probe.lock().expect("probe").calls;
    let bytes = std::fs::read(snapshot.path()).expect("private file");
    let mut document = ferritecad_document::Document::open(snapshot.path()).expect("private file");
    document
        .write_changes(&[ferritecad_document::EditChange::constant(
            f.feature,
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
            30.,
        )
        .expect("value")])
        .expect("change private catalogue");
    document.close().expect("close");
    for (changed, cancelled) in [(true, false), (false, false), (false, true)] {
        if !changed {
            std::fs::write(snapshot.path(), &bytes).expect("restore exact private bytes");
        }
        let (send, receive) = mpsc::channel();
        let cancel = CancelToken::new();
        if cancelled {
            cancel.cancel();
        }
        port.draw(
            Address {
                tab: TabId::default(),
                generation: 1,
            },
            owner::Target::Switch(Arc::clone(&snapshot)),
            cancel,
            move |reply| {
                send.send(reply).expect("reply");
            },
        );
        let reply = receive
            .recv_timeout(Duration::from_secs(3))
            .expect("answer");
        if changed {
            let error = reply.expect_err("changed accepted catalogue");
            assert_eq!(error.kind(), ErrorKind::Input);
            assert!(error.to_string().contains("accepted model changed"));
        } else if cancelled {
            assert_eq!(
                reply.expect_err("cancelled switch").kind(),
                ErrorKind::Cancellation
            );
        } else {
            assert_eq!(
                reply
                    .expect("restored catalogue")
                    .edit_source
                    .expect("source")
                    .version,
                snapshot.version()
            );
        }
        let state = probe.lock().expect("probe");
        assert_eq!(state.calls, before);
        assert_eq!(state.live, 1);
    }
}
