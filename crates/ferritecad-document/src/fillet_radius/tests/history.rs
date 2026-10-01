// SPDX-License-Identifier: MIT
//! §28L: the history of up to four Fillets over one plate, read by the one
//! reader, judged by the one policy and written by the same writers. No
//! kernel: every document is written by the shipped preparation and writer.
use super::*;
use crate::MIN_RADIUS_MM;

/// The sides of [`PLATE`]: Line `i` runs from stored corner `i` to `i + 1`.
const LONG: f64 = 37.5;
const SHORT: f64 = 12.25;

fn side(line: usize) -> f64 {
    if line % 2 == 0 { LONG } else { SHORT }
}

/// The Line two stored corners share, if they are adjacent: corner `j` is
/// where Line `j - 1` ends and Line `j` starts.
fn shared_line(a: usize, b: usize) -> Option<usize> {
    if (a + 1) % 4 == b {
        Some(a)
    } else if (b + 1) % 4 == a {
        Some(b)
    } else {
        None
    }
}

fn bare() -> (tempfile::TempDir, Document, ObjectId) {
    bare_on(PLATE)
}

fn base_extrude(d: &Document) -> ObjectId {
    d.objects()
        .expect("objects")
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
        .expect("the base Extrude")
        .id
}

/// Rounds stored corner `at` with `r`, by the shipped preparation and writer.
fn add(d: &mut Document, body: ObjectId, at: usize, r: f64) -> Result<ObjectId> {
    let objects = d.objects()?;
    let base = base_extrude(d);
    let sketch = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Sketch(s) => Some(s.clone()),
            _ => None,
        })
        .expect("the Sketch");
    let corners = crate::rectangle_corners(base, &sketch)?;
    let prepared = crate::prepare_edge_fillet(
        d,
        body,
        &EdgeFillet {
            edge: FilletEdge {
                feature: base,
                joint: corners[at].joint,
            },
            radius_mm: r,
        },
    )?;
    let id = prepared.feature().id;
    d.write_edge_fillet(&prepared)?;
    Ok(id)
}

/// A plate rounded at `spec`'s corners, in order, with their radii.
fn rounded(
    plate: [[f64; 2]; 4],
    spec: &[(usize, f64)],
) -> (tempfile::TempDir, Document, ObjectId, Vec<ObjectId>) {
    let (root, mut d, body) = bare_on(plate);
    let ids = spec
        .iter()
        .map(|&(at, r)| add(&mut d, body, at, r).expect("added"))
        .collect();
    (root, d, body, ids)
}

fn read(d: &Document) -> Vec<SavedFillet> {
    saved_fillets(d, &d.objects().expect("objects")).expect("read")
}

/// The reader reads none to four Fillets, in the order of the real
/// predecessor chain, each with every other Fillet as a neighbour and the
/// Line it shares with that one — or none, across the plate. Every Fillet
/// carries the seven names of §28A and one origin name per earlier Fillet
/// (7, 15, 24 and 34 in all, beside the Extrude's two caps), the document
/// validates, and the older single-neighbour projection exists for two
/// Fillets only.
#[test]
fn the_reader_reads_none_to_four_fillets_in_history_order() {
    let (_root, mut d, body) = bare();
    let lines = plate_lines(&d).1;
    assert!(read(&d).is_empty());
    let order = [1_usize, 3, 0, 2];
    let radii = [2.375, 3.0625, 1.5, 2.0];
    let mut ids: Vec<ObjectId> = Vec::new();
    for (k, (&at, &r)) in order.iter().zip(&radii).enumerate() {
        ids.push(add(&mut d, body, at, r).expect("added"));
        let saved = read(&d);
        assert_eq!(saved.len(), k + 1);
        for (i, s) in saved.iter().enumerate() {
            assert_eq!(s.feature, ids[i]);
            assert_eq!((s.history_index, s.history_len()), (i + 1, k + 1));
            let base = s.base_feature;
            assert_eq!(s.previous, if i == 0 { base } else { ids[i - 1] });
            assert_eq!(
                s.edge.feature, base,
                "the edge is the plate's, never a Fillet's"
            );
            assert_eq!(s.radius_mm, radii[i]);
            assert_eq!(s.neighbours.len(), k);
            assert_eq!(s.neighbour().is_some(), k == 1, "the old projection");
            for n in &s.neighbours {
                let at_other = order[n.history_index - 1];
                assert_eq!(n.feature, ids[n.history_index - 1]);
                assert_eq!(n.radius_mm, radii[n.history_index - 1]);
                match (n.shared, shared_line(order[i], at_other)) {
                    (Some((line, length)), Some(index)) => {
                        assert_eq!((line, length), (lines[index], side(index)));
                    }
                    (None, None) => {}
                    other => panic!("corners {} and {at_other}: {other:?}", order[i]),
                }
            }
        }
        assert_eq!(
            d.topology_refs().expect("refs").len(),
            2 + 7 * (k + 1) + k * (k + 1) / 2,
            "seven names each and one origin name per earlier Fillet"
        );
        assert!(d.validate().expect("validate").is_ok());
        // Every row of the radius catalogue is editable; the form's list of
        // candidates shrinks to the corners still sharp.
        let objects = d.objects().expect("objects");
        assert!(
            fillet_radius_choices(&d, &objects)
                .iter()
                .all(|c| c.refusal.is_none())
        );
        let choice = crate::fillet_choices(&d, &objects)[0].clone();
        match choice.target {
            Some(t) => {
                assert!(k < 3, "a fourth Fillet leaves no corner");
                assert_eq!(t.corners.len(), 3 - k);
                assert_eq!(t.fillets.len(), k + 1);
                assert_eq!(t.previous_feature, ids[k]);
            }
            None => {
                assert_eq!(k, 3);
                let reason = choice.refusal.expect("a reason");
                assert!(reason.contains("every corner"), "{reason}");
            }
        }
    }
    // A fifth Fillet is refused by name, and nothing is written.
    let before = cells(&d);
    let e = add(&mut d, body, 1, 1.0).expect_err("a fifth");
    assert_eq!(e.kind(), ErrorKind::Unsupported);
    assert_eq!(cells(&d), before);
    let e = add(&mut d, body, 0, 1.0).expect_err("a corner twice");
    assert_eq!(e.kind(), ErrorKind::Unsupported);
    assert_eq!(cells(&d), before);
}

/// `x`'s largest radius beside a Fillet of `r_later` on a Line of `length`,
/// by brute force on the very expression the rule states.
fn largest_before(length: f64, r_later: f64) -> f64 {
    let mut g = length - r_later - MIN_RADIUS_MM;
    while r_later > length - g - MIN_RADIUS_MM {
        g = g.next_down();
    }
    while r_later <= length - g.next_up() - MIN_RADIUS_MM {
        g = g.next_up();
    }
    g
}

/// All 24 orders of the four corners of a square plate whose radii are so
/// near its limits that every neighbour binds in turn. For every Fillet the
/// largest radius offered is the **minimum over every adjacent neighbour** —
/// the earlier ones as `L − r − 0.01`, the later ones as the largest radius
/// that still leaves the later radius its flat — is accepted, and the next
/// float above it is refused. Opposite corners never bind.
#[test]
fn every_neighbour_bounds_the_radius_and_the_bound_is_exact() {
    const SQUARE: [[f64; 2]; 4] = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
    let radii = [4.992, 0.5, 4.996, 0.5];
    let mut bound_by_two = 0;
    for order in permutations([0, 1, 2, 3]) {
        let spec: Vec<(usize, f64)> = order.iter().copied().zip(radii).collect();
        let (_root, d, _body, _ids) = rounded(SQUARE, &spec);
        for (i, s) in read(&d).iter().enumerate() {
            let mut want = 0.5 * 10.0;
            let mut binding = 0;
            for (j, &other) in order.iter().enumerate() {
                if shared_line(order[i], other).is_none() {
                    continue;
                }
                let pair = if j < i {
                    10.0 - radii[j] - MIN_RADIUS_MM
                } else {
                    largest_before(10.0, radii[j])
                };
                binding += usize::from(pair < 5.0);
                want = f64::min(want, pair);
            }
            if binding == 2 {
                bound_by_two += 1;
            }
            let m = s.max_radius_mm().expect("unconstrained");
            assert_eq!(m, want, "order {order:?}, Fillet {}", i + 1);
            s.check_radius(m).expect("the offered maximum is accepted");
            let e = s
                .check_radius(m.next_up())
                .expect_err("the next float is refused");
            assert_eq!(e.kind(), ErrorKind::Input);
            // Through the editor the same number is accepted, and the writer
            // takes it.
            let at = s.feature;
            prepare_fillet_radius(&d, at, m).expect("the maximum is prepared");
            assert!(prepare_fillet_radius(&d, at, m.next_up()).is_err());
        }
    }
    assert!(bound_by_two > 0, "some corner is bound by two neighbours");
}

fn permutations(items: [usize; 4]) -> Vec<[usize; 4]> {
    let mut all = Vec::new();
    for a in 0..4 {
        for b in 0..4 {
            for c in 0..4 {
                for e in 0..4 {
                    let p = [items[a], items[b], items[c], items[e]];
                    let mut sorted = p;
                    sorted.sort_unstable();
                    if sorted == [0, 1, 2, 3] {
                        all.push(p);
                    }
                }
            }
        }
    }
    assert_eq!(all.len(), 24);
    all
}

/// The same policy for every one of the 24 orders, through the real
/// preparation, writer and evaluator: smaller radii are accepted at every
/// step; with every radius 6.125 mm the first corner that shares a *short*
/// Line with an earlier one is refused — Fillets between them in the history
/// or not — and the refusal names the neighbour's UUID and the shared Line.
#[test]
fn all_orders_of_the_four_corners_answer_to_one_policy() {
    let easy = [2.0, 3.0, 2.5, 1.75];
    let mut refused_at = std::collections::BTreeSet::new();
    for order in permutations([0, 1, 2, 3]) {
        // Accepted at every step, and the evaluator agrees on the stored
        // Lines at each.
        let (_root, d, body, ids) =
            rounded(PLATE, &order.iter().copied().zip(easy).collect::<Vec<_>>());
        let objects = d.objects().expect("objects");
        let drawn = match &objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("Sketch")
            .payload
        {
            ObjectPayload::Sketch(s) => s.curves.clone(),
            _ => unreachable!(),
        };
        for id in &ids {
            crate::evaluable_fillet(&objects, &stored_fillet(&d, *id), Some(&drawn))
                .expect("the evaluator agrees");
        }
        drop((body, ids));

        // The widest radius everywhere.
        let (_root, mut d, body) = bare();
        let lines = plate_lines(&d).1;
        let mut done: Vec<ObjectId> = Vec::new();
        let mut at_step = None;
        for (k, &corner) in order.iter().enumerate() {
            let before = cells(&d);
            let guilty = order[..k]
                .iter()
                .position(|&c| shared_line(c, corner).is_some_and(|l| l % 2 == 1));
            match add(&mut d, body, corner, 6.125) {
                Ok(id) => {
                    assert!(guilty.is_none(), "order {order:?}: step {k} should refuse");
                    done.push(id);
                }
                Err(e) => {
                    let j = guilty.unwrap_or_else(|| panic!("order {order:?}: step {k}: {e}"));
                    assert_eq!(e.kind(), ErrorKind::Input);
                    let line = shared_line(order[j], corner).expect("shared");
                    let text = e.to_string();
                    assert!(text.contains("flat"), "{text}");
                    assert!(text.contains(&lines[line].to_string()), "the Line: {text}");
                    assert!(text.contains(&done[j].to_string()), "the Fillet: {text}");
                    assert_eq!(cells(&d), before, "a refusal wrote");
                    at_step = Some(k);
                    break;
                }
            }
        }
        refused_at.insert(at_step);
    }
    // Three corners of a rectangle always hold both ends of one short Line,
    // so no order gets through four widest radii and none is refused at the
    // first corner.
    assert_eq!(
        refused_at,
        std::collections::BTreeSet::from([Some(1), Some(2)]),
        "orders refuse at the second or the third corner"
    );
}

/// The fourth Fillet closes the perimeter: it is judged against the first
/// as well as the third, though two Fillets lie between them in the history.
/// Corner 0 at 6.12 mm leaves 12.25 − 6.12 − 0.01 = 6.12 mm for corner 3 over
/// the short Line they share; the next float is refused naming the first
/// Fillet and that Line, while the long Line to corner 2 owes nothing.
#[test]
fn the_fourth_fillet_closes_the_pair_with_the_first() {
    let (_root, mut d, body, ids) = rounded(PLATE, &[(0, 6.12), (1, 2.0), (2, 2.0)]);
    let lines = plate_lines(&d).1;
    let before = cells(&d);
    let bound = largest_before(SHORT, 6.12);
    assert_eq!(bound, SHORT - 6.12 - MIN_RADIUS_MM);
    let e = add(&mut d, body, 3, bound.next_up()).expect_err("no flat left");
    assert_eq!(e.kind(), ErrorKind::Input);
    let text = e.to_string();
    assert!(
        text.contains(&ids[0].to_string()),
        "the first Fillet: {text}"
    );
    assert!(
        !text.contains(&ids[1].to_string()) && !text.contains(&ids[2].to_string()),
        "not the ones between: {text}"
    );
    assert!(
        text.contains(&lines[3].to_string()),
        "the short Line: {text}"
    );
    assert_eq!(cells(&d), before);
    let fourth = add(&mut d, body, 3, bound).expect("the exact bound");
    let saved = read(&d);
    assert_eq!(saved[3].feature, fourth);
    assert_eq!(saved[3].max_radius_mm(), Some(bound));
    // The first Fillet's own bound now answers to the fourth, a later one
    // across the same Line: the largest first radius that still leaves it.
    let first = &saved[0];
    let m = first.max_radius_mm().expect("unconstrained");
    assert_eq!(m, f64::min(0.5 * SHORT, largest_before(SHORT, bound)));
    assert!(first.check_radius(6.12).is_ok());
}

/// A Fillet outside the class refuses with its kind and reason in the reader
/// and in the evaluator: a corner rounded twice, a fifth Fillet, a branch and
/// a predecessor that is not a feature of the plate. None of them is read as
/// a history of fewer Fillets.
#[test]
fn a_duplicate_a_fifth_a_branch_and_a_foreign_predecessor_are_refused() {
    let (_root, d, _body, ids) = rounded(PLATE, &[(1, 2.0), (3, 3.0), (0, 1.5)]);
    let objects = d.objects().expect("objects");
    let drawn = match &objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("Sketch")
        .payload
    {
        ObjectPayload::Sketch(s) => s.curves.clone(),
        _ => unreachable!(),
    };
    let base = base_extrude(&d);
    let tip = ids[2];
    let template = objects.iter().find(|o| o.id == tip).expect("tip").clone();
    let stored = |id| stored_fillet(&d, id);
    let judge = |extra: &[ObjectRecord], fillet: &Fillet| {
        let mut all = objects.clone();
        all.extend_from_slice(extra);
        crate::evaluable_fillet(&all, fillet, Some(&drawn))
    };
    let forged = |payload: Fillet| {
        let mut r = template.clone();
        r.id = ObjectId::new();
        r.payload = ObjectPayload::Fillet(payload);
        r
    };

    // The sound next Fillet: the last corner, on the third's result.
    let mut fourth = stored(tip);
    fourth.previous = tip;
    fourth.edge.joint = {
        let sketch = match &objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("Sketch")
            .payload
        {
            ObjectPayload::Sketch(s) => s.clone(),
            _ => unreachable!(),
        };
        crate::rectangle_corners(base, &sketch).expect("corners")[2].joint
    };
    fourth.radius_mm = 1.0;
    judge(&[], &fourth).expect("the fourth corner is in the class");

    // A corner rounded twice.
    let mut again = fourth.clone();
    again.edge.joint = stored(ids[1]).edge.joint;
    let e = judge(&[], &again).expect_err("the same corner");
    assert_eq!(e.kind(), ErrorKind::Input);
    assert!(e.to_string().contains(&ids[1].to_string()), "{e}");

    // A fifth: a Fillet on the fourth's result, however its corner is named.
    let fourth_row = forged(fourth.clone());
    let mut fifth = fourth.clone();
    fifth.previous = fourth_row.id;
    let e = judge(std::slice::from_ref(&fourth_row), &fifth).expect_err("a fifth");
    assert_eq!(e.kind(), ErrorKind::Unsupported);
    assert!(e.to_string().contains("at most 4"), "{e}");

    // A branch: another Fillet over the same result.
    let sibling = forged(fourth.clone());
    let fourth_row = forged(fourth.clone());
    let e = judge(&[fourth_row.clone(), sibling], &fourth).expect_err("a branch");
    assert_eq!(e.kind(), ErrorKind::Unsupported);
    assert!(e.to_string().contains("branch"), "{e}");

    // A predecessor that is neither the Extrude nor a Fillet of the plate.
    let mut foreign = fourth.clone();
    foreign.previous = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("Sketch")
        .id;
    let e = judge(&[], &foreign).expect_err("a Sketch as predecessor");
    assert_eq!(e.kind(), ErrorKind::Unsupported);

    // The reader refuses documents with such rows beside the history, by
    // kind: the Fillets must form one chain under the Body's tip, and a plate
    // has four corners.
    let plant = |d: &mut Document, payload: Fillet| {
        let id = ObjectId::new();
        d.write(|w| {
            w.put_object(
                id,
                None,
                100,
                Some("Planted"),
                &ObjectPayload::Fillet(payload),
            )
            .map(|_| ())
        })
        .expect("planted");
        id
    };
    let history = |planted: &[(Option<usize>, Fillet)]| {
        let (root, mut d, _body, ids) = rounded(PLATE, &[(1, 2.0), (3, 3.0), (0, 1.5)]);
        let mut made: Vec<ObjectId> = Vec::new();
        for (over, payload) in planted {
            let mut payload = payload.clone();
            if let Some(k) = over {
                payload.previous = *made.get(*k).unwrap_or(&ids[*k - made.len().min(*k)]);
            }
            made.push(plant(&mut d, payload));
        }
        let objects = d.objects().expect("objects");
        let e = saved_fillets(&d, &objects).expect_err("refused");
        drop(root);
        e
    };
    // A sibling of the third Fillet: two tips.
    let mut sibling = fourth.clone();
    sibling.previous = ids[1];
    let e = history(&[(None, sibling)]);
    assert_eq!(e.kind(), ErrorKind::Unsupported);
    assert!(e.to_string().contains("do not form one chain"), "{e}");
    // A Fillet of a Sketch.
    let mut sketch_fillet = fourth.clone();
    sketch_fillet.previous = plate_lines_sketch(&d);
    let e = history(&[(None, sketch_fillet)]);
    assert_eq!(e.kind(), ErrorKind::Unsupported);
    // A fifth: the fourth and one more.
    let e = history(&[(None, fourth.clone()), (Some(0), fourth.clone())]);
    assert_eq!(e.kind(), ErrorKind::Unsupported);
    assert!(e.to_string().contains("at most 4"), "{e}");
}

fn plate_lines_sketch(d: &Document) -> ObjectId {
    plate_lines(d).0
}

const FOUR: [(usize, f64); 4] = [(1, 2.0), (3, 2.0), (0, 2.0), (2, 2.0)];

/// Under four Fillets the height, the base rectangle and the Line
/// constraints are edited by the same preparations that edit them under one
/// or two, each reading every Fillet in history order; each moves only its
/// own rows, and the rectangle's pair rule holds to the exact float, between
/// Fillets that are not neighbours in the history (corners 1 and 2 are the
/// first and the fourth).
#[test]
fn the_height_the_rectangle_and_the_constraints_keep_all_four_fillets() {
    let (_root, mut d, _body, ids) = rounded(PLATE, &FOUR);
    let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
    let base = base_extrude(&d);
    let listed = |fillets: &[SavedFillet]| fillets.iter().map(|f| f.feature).collect::<Vec<_>>();
    let row = reading
        .features
        .iter()
        .find(|f| f.feature == base)
        .expect("base");
    assert_eq!(row.refusal, None);
    assert_eq!(listed(&row.fillets), ids);
    assert_eq!(listed(&reading.sketches[0].fillets), ids);
    assert_eq!(reading.sketches[0].refusal, None);
    assert_eq!(listed(&reading.constraint_sketches[0].fillets), ids);
    assert_eq!(reading.constraint_sketches[0].refusal, None);

    // The height.
    let objects = d.objects().expect("objects");
    let refs = d.topology_refs().expect("refs");
    let deps = d.dependencies().expect("deps");
    let p = crate::prepare_extrude_height(&d, base, 9.5).expect("prepared");
    assert_eq!(listed(p.fillets()), ids);
    d.write_extrude_height(&p).expect("written");
    assert_eq!(changed_rows(&objects, &d), vec![base]);
    assert_eq!(d.topology_refs().expect("refs"), refs);
    assert_eq!(d.dependencies().expect("deps"), deps);
    assert!(d.validate().expect("validate").is_ok());

    // The rectangle: 20 × 5 holds four radii of 2 mm with 1 mm to spare.
    let objects = d.objects().expect("objects");
    let (sketch, vertices) = starts(&d, rect(false, (0., 0.), (20., 5.0)));
    let p = crate::replace_sketch_coordinates(&d, sketch, &vertices).expect("prepared");
    d.write_sketch_geometry(&p).expect("written");
    assert_eq!(changed_rows(&objects, &d), vec![sketch]);
    assert_eq!(d.topology_refs().expect("refs"), refs);
    // The least depth that leaves the flat between the first and the fourth
    // Fillet (2 + 2 + 0.01), and one float below it: refused, naming the
    // Fillet that is in the way and the Line they share.
    let depth = (4.0 + MIN_RADIUS_MM).max(4.01);
    let exact = {
        let mut least = depth;
        while 2.0 <= crate::fillet::pair_bound(least, 2.0) {
            least = least.next_down();
        }
        least.next_up()
    };
    let (_, v) = starts(&d, rect(false, (0., 0.), (20., exact)));
    crate::replace_sketch_coordinates(&d, sketch, &v).expect("the exact depth");
    let (_, v) = starts(&d, rect(false, (0., 0.), (20., exact.next_down())));
    let e = crate::replace_sketch_coordinates(&d, sketch, &v).expect_err("one float below");
    assert_eq!(e.kind(), ErrorKind::Input);
    let text = e.to_string();
    assert!(text.contains("flat"), "{text}");
    assert!(
        ids.iter().any(|id| text.contains(&id.to_string())),
        "{text}"
    );
    assert!(
        plate_lines(&d)
            .1
            .iter()
            .any(|l| text.contains(&l.to_string())),
        "the Line: {text}"
    );

    // The constraints: only the Sketch row and its capability move.
    let objects = d.objects().expect("objects");
    constrained(&mut d);
    assert_eq!(changed_rows(&objects, &d), vec![sketch]);
    let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
    assert_eq!(listed(&reading.constraint_sketches[0].fillets), ids);
    // A constrained plate's bounds are the solved plate's, never the stored.
    for s in read(&d) {
        assert_eq!(s.max_radius_mm(), None);
        assert!(s.constrained);
    }
    assert!(d.validate().expect("validate").is_ok());
}

/// Every writer re-derives the whole history inside its transaction: a
/// preparation that drops, reorders or changes any of four Fillets, or whose
/// neighbours' radii are not the saved ones, is refused and writes nothing; a
/// radius changed after preparation is refused by the Sketch writer for a
/// rectangle that can no longer hold it, and by the version guard.
#[test]
fn every_writer_rederives_four_fillets_and_refuses_forgery_and_stale_radii() {
    let (_root, mut d, _body, ids) = rounded(PLATE, &FOUR);
    let base = base_extrude(&d);
    let honest = crate::prepare_extrude_height(&d, base, 9.5).expect("prepared");
    let mut forged = Vec::new();
    let mut p = honest.clone();
    p.fillets.truncate(3);
    forged.push(("without the fourth", p));
    let mut p = honest.clone();
    p.fillets[2].radius_mm = 2.5;
    forged.push(("another radius of the third", p));
    let mut p = honest.clone();
    p.fillets.swap(1, 2);
    forged.push(("the second and third swapped", p));
    let mut p = honest.clone();
    p.fillets[3].neighbours.pop();
    forged.push(("the fourth without a neighbour", p));
    let mut p = honest.clone();
    p.fillets.clear();
    forged.push(("no Fillet at all (the legacy writer)", p));
    let before = cells(&d);
    for (why, p) in &forged {
        assert!(d.write_extrude_height(p).is_err(), "{why} was written");
        assert_eq!(cells(&d), before, "{why} wrote something");
    }

    // A radius edit whose neighbours are not the saved ones.
    let honest = prepare_fillet_radius(&d, ids[3], 2.2).expect("prepared");
    let mut stale = honest.clone();
    stale.saved.neighbours[0].radius_mm = 1.0;
    assert!(d.write_fillet_radius(&stale).is_err(), "a stale neighbour");
    let mut dropped = honest.clone();
    dropped.saved.neighbours.pop();
    assert!(d.write_fillet_radius(&dropped).is_err(), "a lost neighbour");
    assert_eq!(cells(&d), before);
    // Another Fillet changes after preparation: the version guard.
    let other = prepare_fillet_radius(&d, ids[0], 2.2).expect("another");
    d.write_fillet_radius(&other).expect("written");
    let e = d.write_fillet_radius(&honest).expect_err("changed");
    assert!(e.to_string().contains("changed"), "{e}");
    assert_eq!(radius_of(&d, ids[3]), 2.0);

    // The Sketch writer reads every current radius: the rectangle prepared
    // for 2 mm radii cannot take the first Fillet's 2.4 mm.
    let (sketch, vertices) = starts(&d, rect(false, (0., 0.), (20., 4.5)));
    let honest = crate::replace_sketch_coordinates(&d, sketch, &vertices).expect("prepared");
    let before = cells(&d);
    let p = prepare_fillet_radius(&d, ids[0], 2.4).expect("fits the saved plate");
    d.write_fillet_radius(&p).expect("radius written");
    let after = cells(&d);
    let e = d.write_sketch_geometry(&honest).expect_err("stale");
    assert!(e.to_string().contains("too large"), "{e}");
    assert_eq!(cells(&d), after, "stale wrote something");
    assert_ne!(after, before);

    // The constraint writer: a Fillet changes after preparation.
    let (sketch, lines) = plate_lines(&d);
    let prepared = crate::prepare_sketch_constraints(
        &d,
        sketch,
        &crate::SketchConstraintEdits {
            remove: Vec::new(),
            add: vec![line_rule(lines[0], crate::LineConstraintKind::Horizontal)],
        },
    )
    .expect("prepared");
    let p = prepare_fillet_radius(&d, ids[2], 1.5).expect("another radius");
    d.write_fillet_radius(&p).expect("written");
    let after = cells(&d);
    assert!(d.write_sketch_constraints(&prepared).is_err());
    assert_eq!(cells(&d), after, "stale wrote something");
}

/// On a constrained plate the evaluator judges all four Fillets, and every
/// adjacent pair, on the Lines the rebuild solved: at the depth that leaves
/// the shared flats the first two pass; one float below it the third (its
/// pair is with the second) and the fourth (its pair is with the **first**)
/// are refused, each naming the Fillet in the way. No second check exists.
#[test]
fn the_evaluator_judges_four_fillets_and_every_pair_on_the_built_lines() {
    let (_root, mut d, _body, ids) = rounded(PLATE, &FOUR);
    constrained(&mut d);
    let (_, lines) = plate_lines(&d);
    let objects = d.objects().expect("objects");
    let judge = |k: usize, depth: f64| {
        crate::evaluable_fillet(
            &objects,
            &stored_fillet(&d, ids[k]),
            Some(&built(&lines, rect(false, (0., 0.), (20., depth)))),
        )
    };
    let mut least = 4.01_f64;
    while 2.0 > crate::fillet::pair_bound(least, 2.0) {
        least = least.next_up();
    }
    while 2.0 <= crate::fillet::pair_bound(least.next_down(), 2.0) {
        least = least.next_down();
    }
    for k in 0..4 {
        judge(k, least).unwrap_or_else(|e| panic!("Fillet {} at {least}: {e}", k + 1));
    }
    let below = least.next_down();
    judge(0, below).expect("the first owes no earlier neighbour");
    judge(1, below).expect("opposite the first");
    let third = judge(2, below).expect_err("the third and the second share a short Line");
    assert_eq!(third.kind(), ErrorKind::Input);
    assert!(third.to_string().contains(&ids[1].to_string()), "{third}");
    assert!(third.to_string().contains(&lines[3].to_string()), "{third}");
    let fourth = judge(3, below).expect_err("the fourth closes the pair with the first");
    assert_eq!(fourth.kind(), ErrorKind::Input);
    assert!(fourth.to_string().contains(&ids[0].to_string()), "{fourth}");
    assert!(
        fourth.to_string().contains(&lines[1].to_string()),
        "{fourth}"
    );
    assert!(
        !fourth.to_string().contains(&ids[1].to_string()),
        "not the opposite one: {fourth}"
    );
    // Each radius judged at its own corner: 3.9 mm deep holds no 2 mm radius.
    for k in 0..4 {
        let e = judge(k, 3.9).expect_err("under 2 r");
        assert_eq!(e.kind(), ErrorKind::Input);
    }
}
