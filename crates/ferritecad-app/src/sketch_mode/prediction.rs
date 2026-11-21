// SPDX-License-Identifier: MIT
//! Predict the rebuild from precisely the coordinates Finish would store.
use ferritecad_document::{EditChange, Point2, Sketch, SketchGeometry};
use ferritecad_types::ObjectId;

const TOLERANCE_MM: f64 = 1e-6;
pub(super) const ELSEWHERE: &str =
    "Finish would not keep the drawing as shown; the solver settles elsewhere.";

fn near(a: Point2, b: Point2) -> bool {
    (a.x - b.x).abs() <= TOLERANCE_MM && (a.y - b.y).abs() <= TOLERANCE_MM
}

fn agrees(a: &SketchGeometry, b: &SketchGeometry) -> bool {
    use SketchGeometry::{Arc, Circle, Line, Point};
    match (a, b) {
        (Point { at: a }, Point { at: b }) => near(*a, *b),
        (Line { start: a, end: e }, Line { start: b, end: f }) => near(*a, *b) && near(*e, *f),
        (
            Circle {
                center: a,
                radius: r,
            },
            Circle {
                center: b,
                radius: s,
            },
        ) => near(*a, *b) && (r - s).abs() <= TOLERANCE_MM,
        (
            Arc {
                center: a,
                radius: r,
                start_angle: s,
                end_angle: e,
            },
            Arc {
                center: b,
                radius: t,
                start_angle: u,
                end_angle: v,
            },
        ) => near(*a, *b) && (r - t).abs() <= TOLERANCE_MM && s == u && e == v,
        _ => a == b,
    }
}

/// The request before geometry writes: retain only surviving curve and rule identities.
pub(super) fn seed(original: Option<&Sketch>, drawing: &Sketch) -> Sketch {
    let mut seed = drawing.clone();
    for c in &mut seed.curves {
        if let Some(saved) = original.and_then(|s| s.curves.iter().find(|s| s.id == c.id)) {
            c.geometry = saved.geometry.clone();
        }
    }
    seed
}

pub(super) fn geometry_changes(
    id: ObjectId,
    original: Option<&Sketch>,
    drawing: &Sketch,
    mut solve: impl FnMut(&Sketch) -> Result<Option<Sketch>, String>,
) -> Result<Vec<EditChange>, String> {
    let mut seed = seed(original, drawing);
    let first = solve(&seed)?;
    let predicted = first.as_ref().unwrap_or(&seed);
    let mut writes = Vec::new();
    for (i, (p, shown)) in predicted.curves.iter().zip(&drawing.curves).enumerate() {
        if !agrees(&p.geometry, &shown.geometry)
            && original.is_some_and(|s| s.curves.iter().any(|c| c.id == shown.id))
        {
            writes.push((i, shown));
        }
    }
    for (i, c) in &writes {
        seed.curves[*i].geometry = c.geometry.clone();
    }
    let last = if writes.is_empty() {
        first
    } else {
        solve(&seed)?
    };
    let answer = last.as_ref().unwrap_or(&seed);
    if answer.curves.len() != drawing.curves.len()
        || !answer
            .curves
            .iter()
            .zip(&drawing.curves)
            .all(|(a, b)| a.id == b.id && agrees(&a.geometry, &b.geometry))
    {
        return Err(ELSEWHERE.into());
    }
    Ok(writes
        .into_iter()
        .map(|(_, c)| EditChange::SetCurveGeometry {
            sketch: id,
            curve: c.id,
            geometry: c.geometry.clone(),
        })
        .collect())
}
