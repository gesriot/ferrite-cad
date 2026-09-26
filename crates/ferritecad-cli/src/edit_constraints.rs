// SPDX-License-Identifier: MIT
//! Text/JSON prepare one typed request; jobs owns solver/copy/publication.
use clap::Args;
use ferritecad_document::{
    AddLineConstraint, AddSketchConstraint, CircleConstraintKind, CircleRadiusMm, Document,
    DocumentVersion, LineConstraintKind, LineEndpoint, LineLengthMm, LineRelation,
    SketchConstraintEdits, SketchCoordinateMm,
};
use ferritecad_jobs::{
    EditSketchConstraintsRequest, EditedSketchConstraints, edit_sketch_constraints_copy,
};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, ContentHash, ObjectId, Result, StableEntityId};
use serde::Deserialize;
use std::{io::Read, path::PathBuf, process::ExitCode};
#[derive(Debug, Args)]
pub struct EditConstraintsArgs {
    /// Saved native XY Line Sketch document; the source remains unchanged.
    source: PathBuf,
    #[arg(long)]
    sketch: ObjectId,
    /// Full content_version from the same inspect snapshot as all chosen UUIDs.
    #[arg(long)]
    expect_version: ContentHash,
    /// Request v1: remove exact constraint UUIDs, then add Line H/V, length in mm,
    /// one Fixed Line endpoint at explicit X/Y mm, equal length, parallel or
    /// perpendicular between two Lines, or — on a Sketch that is one analytic
    /// Circle, or two making an annulus — a radius in mm per Circle, a Fixed
    /// centre at explicit X/Y mm, and a concentricity naming both Circles.
    #[arg(long)]
    request: PathBuf,
    /// New FCAD destination; no overwrite and no --force.
    #[arg(short, long)]
    output: PathBuf,
    /// JSON v1; 0 published, 2 refused, 7 report lost. Usage remains text.
    #[arg(long)]
    json: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_version: u32,
    remove: Vec<StableEntityId>,
    add: Vec<Addition>,
}
/// Which point a `fixed` addition pins.
///
/// A Line endpoint or the centre of a circle, never the `at` of point geometry
/// and never one borrowed for the other: a request that spelled a centre as
/// `start` would pin whichever point a later reader thought that meant.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum At {
    Start,
    End,
    Center,
}
impl At {
    /// The Line endpoint this names, or `None` when it names a centre.
    fn endpoint(&self) -> Option<LineEndpoint> {
        match self {
            Self::Start => Some(LineEndpoint::Start),
            Self::End => Some(LineEndpoint::End),
            Self::Center => None,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "rule", rename_all = "snake_case", deny_unknown_fields)]
enum Addition {
    Horizontal {
        curve_id: StableEntityId,
    },
    Vertical {
        curve_id: StableEntityId,
    },
    Distance {
        curve_id: StableEntityId,
        distance_mm: f64,
    },
    Fixed {
        curve_id: StableEntityId,
        at: At,
        x_mm: f64,
        y_mm: f64,
    },
    /// Two whole Lines of the selected Sketch, named in full; neither leads.
    EqualLength {
        a_curve_id: StableEntityId,
        b_curve_id: StableEntityId,
    },
    /// The same two named sides, keeping a relative orientation instead of a
    /// length. Neither leads here either, so neither can be left out.
    Parallel {
        a_curve_id: StableEntityId,
        b_curve_id: StableEntityId,
    },
    Perpendicular {
        a_curve_id: StableEntityId,
        b_curve_id: StableEntityId,
    },
    /// The radius of the named analytic Circle, in mm.
    ///
    /// Its own rule rather than `distance` reused: a distance is between two
    /// points and a radius is a circle's own scalar, and one request that could
    /// spell either would be one field away from giving a circle a length it
    /// does not have.
    Radius {
        curve_id: StableEntityId,
        radius_mm: f64,
    },
    /// The two named analytic Circles of an annular profile keep one centre.
    ///
    /// Both named in full and neither leading, like the two Line-pair rules: a
    /// shared centre is a relationship, and a request that named one circle
    /// would leave the reader to pick the other. It carries no number, because
    /// "the same place" is not a quantity, and no `at`, because the only point
    /// a circle could mean here is its centre.
    Concentric {
        a_curve_id: StableEntityId,
        b_curve_id: StableEntityId,
    },
}
impl Addition {
    fn checked(self) -> Result<AddSketchConstraint> {
        // The two circle forms are decided first, because neither is a Line
        // addition and neither may be built through the Line vocabulary below.
        match self {
            Self::Radius {
                curve_id,
                radius_mm,
            } => {
                return Ok(AddSketchConstraint::Circle {
                    curve: curve_id,
                    kind: CircleConstraintKind::Radius(CircleRadiusMm::new(radius_mm)?),
                });
            }
            Self::Fixed {
                curve_id,
                at: At::Center,
                x_mm,
                y_mm,
            } => {
                return Ok(AddSketchConstraint::Circle {
                    curve: curve_id,
                    kind: CircleConstraintKind::FixedCenter {
                        x: SketchCoordinateMm::new(x_mm)?,
                        y: SketchCoordinateMm::new(y_mm)?,
                    },
                });
            }
            Self::Concentric {
                a_curve_id,
                b_curve_id,
            } => {
                return Ok(AddSketchConstraint::Concentric {
                    a: a_curve_id,
                    b: b_curve_id,
                });
            }
            _ => {}
        }
        self.line().map(AddSketchConstraint::Line)
    }

    fn line(self) -> Result<AddLineConstraint> {
        let (curve, kind) = match self {
            Self::Horizontal { curve_id } => (curve_id, LineConstraintKind::Horizontal),
            Self::Vertical { curve_id } => (curve_id, LineConstraintKind::Vertical),
            Self::Distance {
                curve_id,
                distance_mm,
            } => (
                curve_id,
                LineConstraintKind::Distance(LineLengthMm::new(distance_mm)?),
            ),
            Self::Fixed {
                curve_id,
                at,
                x_mm,
                y_mm,
            } => (
                curve_id,
                LineConstraintKind::Fixed {
                    // A centre was taken above, so what is left is an endpoint.
                    at: at.endpoint().ok_or_else(|| {
                        CadError::input("a fixed centre belongs to a circle, not to a Line")
                    })?,
                    x: SketchCoordinateMm::new(x_mm)?,
                    y: SketchCoordinateMm::new(y_mm)?,
                },
            ),
            Self::EqualLength {
                a_curve_id,
                b_curve_id,
            } => {
                return Ok(AddLineConstraint::EqualLength {
                    a: a_curve_id,
                    b: b_curve_id,
                });
            }
            Self::Parallel {
                a_curve_id,
                b_curve_id,
            } => {
                return Ok(AddLineConstraint::Relation {
                    a: a_curve_id,
                    b: b_curve_id,
                    relation: LineRelation::Parallel,
                });
            }
            Self::Perpendicular {
                a_curve_id,
                b_curve_id,
            } => {
                return Ok(AddLineConstraint::Relation {
                    a: a_curve_id,
                    b: b_curve_id,
                    relation: LineRelation::Perpendicular,
                });
            }
            // Both circle forms were answered before this, so reaching here
            // with one would be a bug rather than a request to interpret.
            Self::Radius { .. } | Self::Concentric { .. } => {
                return Err(CadError::input(
                    "a radius and a shared centre belong to circles, not to a Line",
                ));
            }
        };
        Ok(AddLineConstraint::Line { curve, kind })
    }
}
fn result(args: &EditConstraintsArgs) -> Result<EditedSketchConstraints> {
    if args.json {
        for p in [&args.source, &args.request, &args.output] {
            crate::json::require_utf8_path(p)?;
        }
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&args.request)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(CadError::input("constraint request exceeds 65536 bytes"));
    }
    // One strict decode of the original bytes: the derived shapes refuse
    // unknown and duplicate keys at every level, escaped duplicates included.
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| CadError::input(format!("invalid constraint request JSON: {e}")))?;
    // The derives also take a request from a JSON array in field order, and a
    // tagged addition from an array led by its rule (§27G). Neither is a
    // request anybody wrote; only the shape is read here, the request itself
    // is `input`.
    let objects = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(serde_json::Value::Object(map)) => map
            .get("add")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|all| all.iter().all(serde_json::Value::is_object)),
        _ => false,
    };
    if !objects {
        return Err(CadError::input(
            "invalid constraint request JSON: the request and each addition must be objects",
        ));
    }
    if input.request_version != 1 {
        return Err(CadError::unsupported(
            "unsupported constraint request_version; expected 1",
        ));
    }
    let edits = SketchConstraintEdits {
        remove: input.remove,
        add: input
            .add
            .into_iter()
            .map(Addition::checked)
            .collect::<Result<_>>()?,
    };
    let document = Document::open_read_only(&args.source)?;
    let expected = DocumentVersion {
        document_id: document.meta().document_id,
        content: args.expect_version,
    };
    document.close()?;
    let request = EditSketchConstraintsRequest {
        source: args.source.clone(),
        expected,
        sketch: args.sketch,
        destination: args.output.clone(),
        edits,
    };
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    edit_sketch_constraints_copy(&request, &mut kernel, &OperationContext::default())
}
pub fn run(args: EditConstraintsArgs) -> Result<ExitCode> {
    let result = result(&args);
    if args.json {
        Ok(crate::json::emit(
            crate::json::Operation::EditSketchConstraintsCopy,
            result.map(crate::json::constraints::Published::from),
        ))
    } else {
        let r = result?;
        println!(
            "saved {} ({}, sketch {})",
            r.destination.display(),
            r.document_id,
            r.sketch
        );
        for c in r.added {
            println!("added constraint {}", c.id);
        }
        for id in r.removed {
            println!("removed constraint {id}");
        }
        match &r.solve {
            Some(solve) => println!("degrees of freedom: {}", solve.degrees_of_freedom()),
            // Nothing left to solve, so nothing to report about it. Said out
            // loud rather than printed as a zero, which would claim the
            // drawing cannot move.
            None => println!("degrees of freedom: not measured; no constraints remain"),
        }
        Ok(ExitCode::SUCCESS)
    }
}
