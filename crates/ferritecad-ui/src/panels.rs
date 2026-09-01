// SPDX-License-Identifier: MIT
//! What the interface offers, drawn but not owned.
//!
//! Each panel is a function over a [`egui::Ui`] that returns what the user
//! asked for, and nothing else. No panel holds a camera, a document or a
//! renderer, and none of them applies what it returns: the caller does that
//! through the reducer, which is where the rules about what a request means
//! already live and are already tested.
//!
//! That split is what keeps a panel from becoming a second place where the
//! camera moves.

use ferritecad_viewport::{PickId, StandardView};

/// What the toolbar was asked for while it was on screen.
///
/// A record of what was pressed, applied by nobody here. Two fields rather
/// than one enum because a frame in which the user pressed a view button is
/// not a frame in which they cannot also have pressed anything else, and a
/// panel that could only report one thing would decide which to drop.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Chosen {
    /// A direction to look from.
    pub view: Option<StandardView>,
    /// The user wants to open a different document.
    pub open: bool,
    /// The user has stopped waiting for the document being read.
    pub cancel: bool,
    /// The user wants to see what is chosen.
    pub frame: bool,
    /// The user wants to see the whole model.
    pub frame_all: bool,
    /// The user wants to stop drawing what is chosen.
    pub hide: bool,
    /// The user wants to stop drawing everything except what is chosen.
    pub isolate: bool,
    /// The user wants the last change to what is drawn taken back.
    pub undo_visibility: bool,
    /// The user wants the other projection.
    pub projection: bool,
    /// The user wants everything drawn again.
    pub show_all: bool,
    /// A change to what is drawn, asked for from a row of the list and named
    /// by the picture that drew it.
    pub row_visibility: Option<RowVisibility>,
    /// A definition the user picked out of the list, by its place in it.
    ///
    /// A position in this frame's list and nothing more: the caller turns it
    /// into an identity by asking the picture, and it means nothing once that
    /// picture is replaced.
    pub definition: Option<usize>,
}

/// What is chosen, in the words a document would use for it.
///
/// Deliberately not the scene's own type: a panel has no business knowing what
/// a document or a kernel is, and the conversion happens where both are
/// already in hand. What it does know is which facts exist, so a caller cannot
/// hand it a number that means something only to this frame.
///
/// # Portable terms only
///
/// Everything the application assigns to these roles outlives the picture it
/// was read from. There is no role for a pick, mesh index, face index, shape
/// handle, session or occurrence position. The strings are borrowed display
/// text rather than proof of their own provenance, so the application boundary
/// remains responsible for supplying the source's leaf file name rather than
/// its path. See [`Selected::rows`], which is what actually reaches a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selected<'a> {
    /// A body of the open document.
    Body {
        name: Option<&'a str>,
        /// The identifier the document stores for it.
        object: &'a str,
    },
    /// One face of a body, named by what the document stores about it.
    ///
    /// Only durable terms reach here. A face the document does not name is not
    /// selectable as a face at all, so there is no case for one.
    Face {
        /// What the body is called, if the document called it anything.
        name: Option<&'a str>,
        /// The identifier the document stores for the body.
        object: &'a str,
        /// Every stored reference that names exactly this face, in the order
        /// the document stores them. More than one is normal and is shown as
        /// more than one: which of them is "the" name is not this window's
        /// decision to make.
        names: &'a [TopologyName<'a>],
    },
    /// One topological edge of a native body, named exactly by the document.
    ///
    /// Only durable terms reach here, exactly as for a face: an edge the
    /// document does not name is not selectable as an edge at all.
    Edge {
        /// What the body is called, if the document called it anything.
        name: Option<&'a str>,
        /// The identifier the document stores for the body.
        object: &'a str,
        /// Every stored reference that names exactly this edge, in the order
        /// the document stores them.
        names: &'a [TopologyName<'a>],
    },
    /// One topological vertex of a native body, named exactly by the
    /// document.
    ///
    /// Only durable terms reach here, exactly as for a face and an edge: a
    /// corner the document does not name is not selectable as a corner at all.
    Vertex {
        /// What the body is called, if the document called it anything.
        name: Option<&'a str>,
        /// The identifier the document stores for the body.
        object: &'a str,
        /// Every stored reference that names exactly this corner, in the order
        /// the document stores them.
        names: &'a [TopologyName<'a>],
    },
    /// A definition inside a file this document imported.
    Imported {
        name: Option<&'a str>,
        /// The file it came from, already reduced to a name by the caller.
        source_file: Option<&'a str>,
        /// The identifier the document gave those bytes.
        source: &'a str,
        /// The name the file itself gave the definition.
        definition_key: &'a str,
        solids: Option<u32>,
        /// Why the imported geometry is unavailable, if it was deliberately
        /// retained without triangles.
        geometry_unavailable: Option<GeometryUnavailable<'a>>,
    },
}

/// Display-ready facts about imported geometry the current viewer cannot draw.
///
/// This is deliberately only text chosen by the application. It carries no
/// document diagnostic, kernel error, handle, path, pick or scene identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeometryUnavailable<'a> {
    /// The source-local entity the persisted validation finding named.
    pub finding_entity: &'a str,
    /// The stored import-time validation finding, in human-readable words.
    pub validation: &'a str,
    /// Why the current tessellator still cannot produce complete geometry.
    pub tessellation: &'a str,
}

/// One durable name for an entity, already turned into words by the caller.
///
/// Text rather than document types, for the same reason the rest of this
/// module deals in text: a panel that could name a `SemanticRole` would be a
/// panel that knows what a document is.
///
/// One type for a face, an edge and a vertex. What a document stores about any
/// of them is the same six terms, and three structures would be three formats
/// to keep in step; which kind is meant is already in `expected_kind` and in
/// the role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopologyName<'a> {
    /// The identifier the document stores for the reference itself.
    pub reference: &'a str,
    /// The object holding the reference.
    pub owner: &'a str,
    /// The feature whose output it names.
    pub producer_feature: &'a str,
    /// The kind of entity it expects.
    pub expected_kind: &'a str,
    /// What the named entity is, semantically.
    pub role: &'a str,
    /// How many entities it selects, and which.
    pub rule: &'a str,
}

/// What a document calls one face. See [`TopologyName`].
pub type FaceName<'a> = TopologyName<'a>;

/// What a document calls one edge. See [`TopologyName`].
pub type EdgeName<'a> = TopologyName<'a>;

/// What a document calls one topological vertex. See [`TopologyName`].
pub type VertexName<'a> = TopologyName<'a>;

/// Every stored name, in the order the document stores them.
///
/// One statement for a face, an edge and a vertex: what the document says
/// about any of them is the same six terms, and three copies of this loop
/// would be three formats to keep in step.
fn push_names(rows: &mut Vec<(&'static str, String)>, names: &[TopologyName<'_>]) {
    for name in names {
        rows.push(("Reference", name.reference.to_owned()));
        rows.push(("Owner", name.owner.to_owned()));
        rows.push(("Feature", name.producer_feature.to_owned()));
        rows.push(("Entity", name.expected_kind.to_owned()));
        rows.push(("Role", name.role.to_owned()));
        rows.push(("Rule", name.rule.to_owned()));
    }
}

impl Selected<'_> {
    /// The lines to put on screen, in order.
    ///
    /// Built as data rather than drawn directly so that what a person will
    /// read can be examined without a window. A fact the loader did not find
    /// is left out rather than shown as blank or invented.
    pub fn rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = Vec::new();
        match self {
            Self::Body { name, object } => {
                rows.push(("Kind", "Body".to_owned()));
                if let Some(name) = name {
                    rows.push(("Name", (*name).to_owned()));
                }
                rows.push(("Object", (*object).to_owned()));
            }
            Self::Face {
                name,
                object,
                names,
            } => {
                rows.push(("Kind", "Face".to_owned()));
                if let Some(name) = name {
                    rows.push(("Body", (*name).to_owned()));
                }
                rows.push(("Object", (*object).to_owned()));
                push_names(&mut rows, names);
            }
            Self::Edge {
                name,
                object,
                names,
            } => {
                rows.push(("Kind", "Edge".to_owned()));
                if let Some(name) = name {
                    rows.push(("Body", (*name).to_owned()));
                }
                rows.push(("Object", (*object).to_owned()));
                push_names(&mut rows, names);
            }
            Self::Vertex {
                name,
                object,
                names,
            } => {
                rows.push(("Kind", "Vertex".to_owned()));
                if let Some(name) = name {
                    rows.push(("Body", (*name).to_owned()));
                }
                rows.push(("Object", (*object).to_owned()));
                push_names(&mut rows, names);
            }
            Self::Imported {
                name,
                source_file,
                source,
                definition_key,
                solids,
                geometry_unavailable,
            } => {
                rows.push(("Kind", "Imported definition".to_owned()));
                if let Some(name) = name {
                    rows.push(("Name", (*name).to_owned()));
                }
                if let Some(file) = source_file {
                    rows.push(("File", (*file).to_owned()));
                }
                rows.push(("Source", (*source).to_owned()));
                // The file's own name for it, which is what makes it findable
                // again in those bytes and nowhere else.
                rows.push(("Definition", (*definition_key).to_owned()));
                if let Some(solids) = solids {
                    rows.push(("Solids", solids.to_string()));
                }
                if let Some(unavailable) = geometry_unavailable {
                    rows.push(("Geometry", "Imported geometry unavailable".to_owned()));
                    rows.push(("Finding entity", unavailable.finding_entity.to_owned()));
                    rows.push(("Validation", unavailable.validation.to_owned()));
                    rows.push(("Tessellation", unavailable.tessellation.to_owned()));
                }
            }
        }
        rows
    }

    /// One line naming this definition, for a list of them.
    ///
    /// Name and portable identity together. Two definitions may be called the
    /// same thing – a document may hold two bodies called `Plate`, and two
    /// files may each describe a `Bracket` – so a line that gave only the name
    /// would offer no way to tell which row is which.
    pub fn summary(&self) -> String {
        match self {
            Self::Body { name, object } => match name {
                Some(name) => format!("{name} · {object}"),
                None => format!("Body · {object}"),
            },
            // A list of definitions holds no faces, edges or corners, so this
            // is what any of them would be called if one ever reached a row:
            // the body it is part of, said the same way.
            Self::Face { name, object, .. }
            | Self::Edge { name, object, .. }
            | Self::Vertex { name, object, .. } => match name {
                Some(name) => format!("{name} · {object}"),
                None => format!("Body · {object}"),
            },
            Self::Imported {
                name,
                definition_key,
                source_file,
                source,
                geometry_unavailable,
                ..
            } => {
                let named = match name {
                    Some(name) => format!("{name} · {definition_key}"),
                    None => (*definition_key).to_owned(),
                };
                let described = match source_file {
                    // A useful hint, but not identity: unrelated files in
                    // different directories commonly share one basename.
                    Some(file) => format!("{named} · {file}"),
                    None => named,
                };
                // The key is scoped to this source. Always include both so
                // two unrelated files with the same name and ordinary STEP
                // numbering cannot produce rows that read alike.
                let described = format!("{described} · {source}");
                if geometry_unavailable.is_some() {
                    format!("{described} · geometry unavailable")
                } else {
                    described
                }
            }
        }
    }
}

/// Every definition in the picture, and the one that is chosen.
///
/// Read-only, like the inspector: pressing a row asks for a definition to be
/// chosen and changes nothing itself. Returns the row that was pressed, which
/// the caller turns into an identity by asking the picture – a row's position
/// is how a list reports a press and is not a name for anything.
///
/// One row per definition, in the order the picture packs them. Two imported
/// objects that draw the same definition contribute one row between them,
/// because they contribute one definition between them.
pub fn definitions_panel(
    ui: &mut egui::Ui,
    definitions: &[Selected<'_>],
    chosen: Option<usize>,
    offers: &[RowVisibility],
) -> Rows {
    let mut rows = Rows::default();
    if definitions.is_empty() {
        // A document with nothing in it says so. Offering an empty list with
        // no explanation would look like a list that failed to load.
        ui.label("No definitions");
        return rows;
    }

    egui::ScrollArea::vertical()
        .max_height(140.0)
        .show(ui, |ui| {
            for (row, definition) in definitions.iter().enumerate() {
                // Every definition, whether or not it is being drawn: a list
                // that dropped what was hidden would be a list with no way
                // back to it, and the hidden ones are exactly what a person
                // is looking for when they wonder where something went.
                let offer = offers.get(row).copied().unwrap_or(RowVisibility::Neither);
                let is_hidden = offer.is_hidden();
                let summary = if is_hidden {
                    format!("{} · hidden", definition.summary())
                } else {
                    definition.summary()
                };
                // Beside the name, a row says what can be done about whether
                // it is drawn. Which of the two that is was decided by the
                // caller, the only thing that knows what the picture draws;
                // this draws what it was given and reports what was pressed.
                let response = ui
                    .horizontal(|ui| {
                        if let Some((label, _)) = offer.control()
                            && ui.small_button(label).clicked()
                        {
                            rows.visibility = Some(offer);
                        }
                        // `selectable_label` draws the chosen one differently,
                        // which is how a click in the viewport shows up here.
                        // A hidden row's is disabled: greyed rather than
                        // missing, so a person can see that the row is there
                        // and that pressing it is not the way back.
                        ui.add_enabled(
                            !is_hidden,
                            egui::Button::selectable(chosen == Some(row), summary),
                        )
                    })
                    .inner;
                // A hidden row reports neither, and the rule is written here
                // rather than left to whether a disabled widget happens to
                // report a click: pressing it would choose geometry nobody can
                // see, and pointing at it would mark nothing on screen.
                if is_hidden {
                    continue;
                }
                if response.clicked() {
                    rows.pressed = Some(row);
                }
                // Separate from pressing, and deliberately so: pointing at a
                // row says which geometry it is, and choosing it is a decision
                // the user has not made yet.
                if response.hovered() {
                    rows.hovered = Some(row);
                }
            }
        });
    rows
}

/// What the list of definitions was asked while it was on screen.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Rows {
    /// A row that was pressed, which is a choice.
    pub pressed: Option<usize>,
    /// A row the pointer is over, which is a question and not a choice.
    pub hovered: Option<usize>,
    /// A change to what is drawn that the user asked for, named by the picture
    /// that drew the list rather than by where the row sits in it.
    ///
    /// An identity rather than a position because this one leaves the panel:
    /// a row number would have to be turned back into a definition by
    /// somebody, and the picture is the only thing that can say what sits
    /// there.
    pub visibility: Option<RowVisibility>,
}

/// What one row offers to do about whether its definition is drawn.
///
/// One value per row rather than a pair of optional controls: a row cannot
/// offer to hide something that is already hidden and to show something that
/// is already drawn, and saying so as one enum means no arrangement of the
/// interface can present both at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowVisibility {
    /// Nothing to offer: this definition draws nothing wherever it is, so
    /// taking it off screen and putting it back are the same picture.
    Neither,
    /// Drawn, and this is what asks for it to be taken off screen.
    Hide(PickId),
    /// Not drawn, and this is what asks for it back.
    Show(PickId),
}

impl RowVisibility {
    /// Whether this row's definition is currently off screen.
    ///
    /// The mark a row carries and the control it offers are two readings of
    /// one fact, so they cannot disagree about which rows are missing.
    fn is_hidden(self) -> bool {
        matches!(self, Self::Show(_))
    }

    /// What the control on this row says, and what pressing it asks for.
    fn control(self) -> Option<(&'static str, PickId)> {
        match self {
            Self::Neither => None,
            Self::Hide(pick) => Some(("Hide", pick)),
            Self::Show(pick) => Some(("Show", pick)),
        }
    }
}

/// Shows what is chosen, and nothing when nothing is.
///
/// Read-only. Choosing happens in the viewport and clearing happens by
/// clicking away from the model; a panel that could change the selection would
/// be a second place where it is decided.
pub fn selection_inspector(ui: &mut egui::Ui, selected: Option<Selected<'_>>) {
    let Some(selected) = selected else {
        // Said rather than left blank: an empty strip is indistinguishable
        // from an interface that has stopped working.
        ui.label("Nothing selected");
        return;
    };

    egui::Grid::new("ferritecad selection")
        .num_columns(2)
        .show(ui, |ui| {
            for (label, value) in selected.rows() {
                ui.label(label);
                ui.add(egui::Label::new(value).truncate());
                ui.end_row();
            }
        });
}

/// What one solve found out about one sketch, in words a window can print.
///
/// Not the scene's type and not the evaluator's: a panel has no business
/// knowing what a document, a rebuild or a solver is, and the conversion
/// happens where all three are already in hand. What arrives here is the same
/// vocabulary the rest of this module deals in – borrowed display text,
/// durable identifiers already turned into strings, and one number.
///
/// # Portable terms only
///
/// There is no field for a `ConstraintId`, a `PointId`, an equation index, a
/// native tag, a session or a position in this frame's list, because a solve
/// report holds none of those and a panel that could print one would be a
/// panel that outlived the solve it describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolvedSketch<'a> {
    /// What the document calls it, when it calls it anything.
    pub name: Option<&'a str>,
    /// The identifier the document stores for the sketch, whole.
    pub object: &'a str,
    /// How much freedom the drawing has left. Zero means it cannot move.
    pub degrees_of_freedom: usize,
    /// The constraints that repeat what the rest already said, named as the
    /// document names them and in the order the document stores them.
    pub redundant: &'a [RedundantExplanation<'a>],
}

/// One repeated constraint, as a person reads it.
///
/// Both halves, because either alone is useless. The identifier is what a
/// person can find the constraint by and is shown whole; the sentence is what
/// it says, already written by whoever had the document open. Nothing here is
/// a shape a programmer prints – no rule value, no variant name, no list –
/// because a panel that fell back on one would be showing its own internals
/// the moment anything else was missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedundantExplanation<'a> {
    /// The identifier the document stores this constraint under, whole.
    pub identifier: &'a str,
    /// What it says: one finished sentence naming the kind of relationship,
    /// the parts of the drawing it is about, and the size it asks for when it
    /// asks for one.
    pub says: &'a str,
}

/// One constraint of a conflict, as a person reads it.
///
/// The same two halves a repeated constraint arrives in, and for the same
/// reason: the identifier is what a person finds the constraint by in their
/// own document, and the sentence is what tells them whether they want to.
/// Nothing here is a value a programmer prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConflictingRule<'a> {
    /// The identifier the document stores this constraint under, whole.
    pub identifier: &'a str,
    /// What it says: one finished sentence, written by whoever had the
    /// document open.
    pub says: &'a str,
}

/// Why an attempt to open a document failed, when the reason has parts.
///
/// This is not about the model on screen. It describes a different document –
/// one that did not open – and it names that document itself so that the two
/// cannot be read as one. The picture, the choice made in it and what the last
/// solve found out are all still whatever they were.
///
/// Not the scene's type and not the evaluator's: this crate knows nothing of
/// documents, rebuilds, solvers or errors, and every field here is text
/// somebody else already wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenFailure<'a> {
    /// The file that was being opened, named as the attempt named it.
    pub file: &'a str,
    /// The identifier the document stores for the sketch whose constraints
    /// disagree, whole.
    pub sketch: &'a str,
    /// The constraints that cannot all hold, in the order they arrived in.
    pub constraints: &'a [ConflictingRule<'a>],
}

/// What a section reporting a failed attempt calls itself.
const OPEN_FAILED: &str = "Open failed";

/// What kind of failure this section is about.
///
/// Said as a row rather than folded into the heading: the heading says an
/// attempt failed, which is one fact, and what went wrong is another.
const CONSTRAINT_CONFLICT: &str = "Constraint conflict";

impl OpenFailure<'_> {
    /// The lines to put on screen, in order.
    ///
    /// Built as data rather than drawn directly, exactly as [`Selected::rows`]
    /// and [`SolvedSketch::rows`] are, and for the same reason.
    pub fn rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = vec![
            ("Problem", CONSTRAINT_CONFLICT.to_owned()),
            // The file that was attempted, always, and never the one on
            // screen: these two rows are the whole of what keeps a reader from
            // taking this for a report about the model in front of them.
            ("File", self.file.to_owned()),
            ("Sketch", self.sketch.to_owned()),
        ];
        for constraint in self.constraints {
            // Two lines rather than one, on the same terms as a repeated
            // constraint: neither half is readable once a narrow row runs them
            // together.
            rows.push(("Constraint", constraint.identifier.to_owned()));
            rows.push(("Says", constraint.says.to_owned()));
        }
        rows
    }
}

/// What the last attempt to open a document failed over, and nothing else.
///
/// Read-only, like every other section here: no row can be chosen, pressing
/// one is not a thing that can happen, and there is nothing to hand back.
///
/// Nothing at all is drawn when there is nothing to report, which is the usual
/// state of a window. A permanent line saying that the last Open did not fail
/// would be a sentence nobody has a use for, and unlike an empty list of
/// definitions there is no risk of reading absence as breakage: a section
/// about a failure appears exactly when one happened.
pub fn open_failure_panel(ui: &mut egui::Ui, failure: Option<OpenFailure<'_>>) {
    let Some(failure) = failure else {
        return;
    };
    ui.label(OPEN_FAILED);
    egui::Grid::new("ferritecad open failure")
        .num_columns(2)
        .show(ui, |ui| {
            for (label, value) in failure.rows() {
                ui.label(label);
                ui.add(egui::Label::new(value).truncate());
                ui.end_row();
            }
        });
    // Drawn by the section itself, so that a window with nothing to report
    // shows one separator under the toolbar rather than two.
    ui.separator();
}

/// What an unnamed sketch is called on screen.
///
/// Said rather than left blank. A row with an empty first line is
/// indistinguishable from a row whose name failed to arrive, and a sketch that
/// vanished from the list entirely because nobody named it is worse than
/// either.
const UNNAMED: &str = "Unnamed sketch";

/// What a section with nothing to report says.
const NOTHING_SOLVED: &str = "No solved constrained sketches";

impl SolvedSketch<'_> {
    /// The lines to put on screen, in order.
    ///
    /// Built as data rather than drawn directly, exactly as [`Selected::rows`]
    /// is and for the same reason: what a person will read can then be
    /// examined without a window.
    pub fn rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = vec![
            ("Sketch", self.name.unwrap_or(UNNAMED).to_owned()),
            ("Object", self.object.to_owned()),
            // Read from the number rather than carried beside it, so a row
            // saying "fully constrained" and a row saying two degrees of
            // freedom cannot appear together.
            ("Status", self.status().to_owned()),
            ("Degrees of freedom", self.degrees_of_freedom.to_string()),
        ];
        if self.redundant.is_empty() {
            // A statement rather than an absent row: no line at all reads as a
            // panel that has not got round to this sketch yet.
            rows.push(("Redundant", "None".to_owned()));
        }
        for constraint in self.redundant {
            // Two lines rather than one: the identifier is what a person
            // searches their document for, and the sentence is what tells them
            // whether they want to. Running them together would leave neither
            // readable once a row is narrow enough to be truncated.
            rows.push(("Redundant", constraint.identifier.to_owned()));
            rows.push(("Says", constraint.says.to_owned()));
        }
        rows
    }

    /// What a person is told about how settled this drawing is.
    fn status(&self) -> &'static str {
        if self.degrees_of_freedom == 0 {
            "Fully constrained"
        } else {
            "Under-constrained"
        }
    }
}

/// What the solve of each constrained sketch found out, and nothing else.
///
/// Read-only, and not an inspector: no sketch is drawn in the viewport, none
/// of these rows can be chosen, and pressing one is not a thing that can
/// happen. The panel returns nothing because there is nothing for a caller to
/// apply – a section that could report a press would be a second place where
/// what is chosen is decided.
///
/// Nothing here asks a solver anything. Every value was found out by the one
/// rebuild that produced the picture and has been carried since; a panel that
/// could solve would solve once per frame.
pub fn sketch_solves_panel(ui: &mut egui::Ui, sketches: &[SolvedSketch<'_>]) {
    ui.label("Sketch solves");
    if sketches.is_empty() {
        // Said rather than left blank, for the same reason an empty list of
        // definitions says so: a section that draws nothing is
        // indistinguishable from one that failed.
        ui.label(NOTHING_SOLVED);
        return;
    }

    // Whatever room is left rather than a fixed height: this section is last
    // and a bound of its own would leave a window with space in it showing
    // fewer sketches than fit. What does not fit is scrolled to, because every
    // sketch that was solved has to be reachable.
    egui::ScrollArea::vertical()
        .id_salt("ferritecad sketch solves")
        .show(ui, |ui| {
            for (index, sketch) in sketches.iter().enumerate() {
                egui::Grid::new(("ferritecad sketch solve", index))
                    .num_columns(2)
                    .show(ui, |ui| {
                        for (label, value) in sketch.rows() {
                            ui.label(label);
                            ui.add(egui::Label::new(value).truncate());
                            ui.end_row();
                        }
                    });
            }
        });
}

/// What the window is doing, as far as the toolbar needs to know.
///
/// `line` is a finished sentence and `progress` is how far through a reading
/// it is. `None` there is not "nought per cent": it is nothing being read,
/// which is what decides whether there is anything to offer to stop.
///
/// Not `#[non_exhaustive]`, unlike [`Chosen`]: this one is an argument, and a
/// caller that cannot name every field cannot pass one at all.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Activity<'a> {
    pub line: &'a str,
    pub progress: Option<f32>,
    /// Whether the picture can say where what is chosen actually is.
    pub can_frame_selection: bool,
    /// Whether the picture has any extent at all. A document with nothing in
    /// it has nowhere to point a camera, and says so by offering nothing.
    pub can_frame_scene: bool,
    /// Whether there is a chosen definition still being drawn. Choosing
    /// something already hidden is not a thing that can happen, so this is
    /// exactly "something is chosen".
    pub can_hide: bool,
    /// Whether anything is hidden and could be brought back.
    pub can_show_all: bool,
    /// Whether there is a chosen definition still being drawn with something
    /// else still drawn beside it. On its own it is isolated already.
    pub can_isolate: bool,
    /// Whether there is a change to what is drawn that could be taken back.
    pub can_undo_visibility: bool,
    /// Whether the model is drawn as a drawing rather than as an eye sees it.
    pub orthographic: bool,
}

/// The toolbar: a document to open, the directions a drawing would name, what
/// the window is doing, and a way to stop it.
///
/// Returns what was pressed. The order of the views is the one a drawing
/// office reads in, and the keyboard shortcuts beside each name are the same
/// ones the window binds, so the panel documents them rather than making the
/// user find out.
///
/// [`Activity::line`] arrives as a finished sentence. What the states are and
/// which one applies is decided where the loading happens; drawing it here
/// rather than composing it here is what keeps one account of what is going
/// on.
pub fn toolbar(ui: &mut egui::Ui, activity: Activity<'_>) -> Chosen {
    let mut chosen = Chosen::default();
    ui.horizontal(|ui| {
        // First, and separated: opening replaces everything else on screen,
        // where the buttons after it only change where it is seen from.
        chosen.open = ui.button("Open…").clicked();
        ui.separator();

        // Offered only when there is something to show, and disabled rather
        // than hidden: a control that comes and goes is harder to find than
        // one that is plainly not available yet.
        chosen.frame = ui
            .add_enabled(
                activity.can_frame_selection,
                egui::Button::new(format!("Frame selected ({FRAME_KEY})")),
            )
            .clicked();
        // Beside it, because the two answer the same question about different
        // things: show me this, and show me everything. Available whenever the
        // picture has an extent, which is what makes it the way back from a
        // camera that has wandered off the model.
        chosen.frame_all = ui
            .add_enabled(
                activity.can_frame_scene,
                egui::Button::new(format!("Frame all ({FRAME_ALL_KEY})")),
            )
            .clicked();
        // Beside the framing buttons, because they are the same kind of
        // thing: ways of seeing what is already there. Hiding removes a part
        // from the picture and from what a click can reach, and putting it
        // back is one press away and always says so.
        chosen.hide = ui
            .add_enabled(
                activity.can_hide,
                egui::Button::new(format!("Hide selected ({HIDE_KEY})")),
            )
            .clicked();
        // Between them, because it is the other way of saying "this one":
        // Hide removes what is chosen, Isolate removes everything else, and
        // Show all is the way back from either.
        chosen.isolate = ui
            .add_enabled(
                activity.can_isolate,
                egui::Button::new(format!("Isolate selected ({ISOLATE_KEY})")),
            )
            .clicked();
        chosen.show_all = ui
            .add_enabled(
                activity.can_show_all,
                egui::Button::new(format!("Show all ({SHOW_ALL_KEY})")),
            )
            .clicked();
        // Last of the four, and deliberately without a shortcut: this takes
        // back a change to what is on screen and nothing else, and a key that
        // people read as "undo" would promise to take back far more than that.
        chosen.undo_visibility = ui
            .add_enabled(
                activity.can_undo_visibility,
                egui::Button::new("Undo visibility"),
            )
            .clicked();
        ui.separator();

        // Named rather than toggled: a control that only said "projection"
        // would leave a person to work out which one they are looking at from
        // the picture, which is exactly what is hard about the two.
        let projection = if activity.orthographic {
            "Orthographic"
        } else {
            "Perspective"
        };
        chosen.projection = ui
            .button(format!("{projection} ({PROJECTION_KEY})"))
            .clicked();
        ui.separator();

        ui.label("View");
        for (view, name, key) in VIEWS {
            if ui.button(format!("{name} ({key})")).clicked() {
                chosen.view = Some(*view);
            }
        }

        // Last, and given whatever room is left: the line is the one thing
        // here that can be any length, and a long file name must push no
        // button off the toolbar.
        ui.separator();
        if let Some(fraction) = activity.progress {
            // Offered only while there is something to stop. A button that is
            // there all the time and does nothing most of the time teaches
            // people not to trust it.
            chosen.cancel = ui.button("Cancel").clicked();
            ui.add(egui::ProgressBar::new(fraction).desired_width(80.0));
        }
        ui.add(egui::Label::new(activity.line).truncate());
    });
    chosen
}

/// Every standard view, with what to call it and the key that reaches it.
///
/// One list, used by the panel here and available to whatever binds the keys,
/// so a shortcut cannot end up printed on a button that does something else.
/// The key that shows what is chosen, printed on its button.
///
/// One place, read by the panel and by whatever binds the keys, for the same
/// reason [`VIEWS`] is one place: a shortcut printed on a button that does
/// something else is worse than no shortcut at all.
pub const FRAME_KEY: &str = "F";

/// The key that shows the whole model, printed on its button.
///
/// Distinct from [`FRAME_KEY`] and from every view key, and checked to be: two
/// actions on one key is a shortcut whose meaning depends on state nobody can
/// see.
pub const FRAME_ALL_KEY: &str = "A";

/// The key that stops drawing what is chosen, printed on its button.
///
/// One place, read by the panel and by whatever binds the keys, for the same
/// reason [`FRAME_KEY`] is one place.
pub const HIDE_KEY: &str = "H";

/// The key that swaps the projection, printed on the button that names it.
///
/// One place, read by the panel and by whatever binds the keys, for the same
/// reason [`FRAME_KEY`] is one place.
pub const PROJECTION_KEY: &str = "O";

/// The key that leaves only what is chosen on screen, printed on its button.
///
/// One place, read by the panel and by whatever binds the keys, for the same
/// reason [`FRAME_KEY`] is one place.
pub const ISOLATE_KEY: &str = "I";

/// The key that draws everything again, printed on its button.
///
/// `U` rather than `S`: `S` is one keystroke from the view keys a drawing
/// office reaches for, and this one undoes something.
pub const SHOW_ALL_KEY: &str = "U";

pub const VIEWS: &[(StandardView, &str, &str)] = &[
    (StandardView::Front, "Front", "1"),
    (StandardView::Back, "Back", "2"),
    (StandardView::Left, "Left", "3"),
    (StandardView::Right, "Right", "4"),
    (StandardView::Top, "Top", "5"),
    (StandardView::Bottom, "Bottom", "6"),
    (StandardView::Isometric, "Iso", "7"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Lays out one string and reports how wide it came out.
    ///
    /// Zero for a build with no font: `egui` lays the text out against whatever
    /// typefaces are installed, and with none installed there is nothing to
    /// measure. That is exactly the failure this guards against, and it is a
    /// quiet one – no error, no panic, just an interface with no words on it.
    fn text_width(context: &egui::Context, text: &str) -> f32 {
        context.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(
                    text.to_owned(),
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                )
                .size()
                .x
        })
    }

    #[test]
    fn this_build_can_draw_words() {
        let context = egui::Context::default();
        // A frame has to have run for the fonts to be loaded. Its texture
        // deltas are the font atlas, and epaint refuses to have them dropped
        // unapplied – a renderer that ignored them would show blank glyphs.
        let mut output = context.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();

        let width = text_width(&context, "Front");
        assert!(
            width > 0.0,
            "no font is installed, so every label in this build is blank"
        );

        // And it is really measuring the glyphs rather than returning a
        // constant: a longer word is wider than a shorter one.
        assert!(
            text_width(&context, "Isometric") > width,
            "text measurement does not depend on the text"
        );
    }

    #[test]
    fn a_label_reaches_the_geometry_that_would_be_drawn() {
        let context = egui::Context::default();

        let mut with_text = context.run_ui(egui::RawInput::default(), |ui| {
            ui.label("Front");
        });
        with_text.textures_delta.clear();
        let mut empty = context.run_ui(egui::RawInput::default(), |_| {});
        empty.textures_delta.clear();

        // End to end, past layout and into what a renderer would upload. A
        // font that laid out but produced no glyphs would pass the measurement
        // above and still draw nothing.
        let vertices = |output: egui::FullOutput| {
            context
                .tessellate(output.shapes, 1.0)
                .into_iter()
                .map(|primitive| match primitive.primitive {
                    egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                    egui::epaint::Primitive::Callback(_) => 0,
                })
                .sum::<usize>()
        };

        let drawn = vertices(with_text);
        assert!(
            drawn > vertices(empty),
            "a label produced no more geometry than an empty frame"
        );
    }

    #[test]
    fn the_toolbar_asks_for_nothing_until_it_is_pressed() {
        let context = egui::Context::default();
        let mut chosen = Chosen::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            chosen = toolbar(ui, Activity::default());
        });
        output.textures_delta.clear();

        // A frame in which the user did nothing must ask for nothing. A
        // toolbar that reported a press every frame would reopen the file
        // dialog for as long as the window was on screen.
        assert_eq!(chosen, Chosen::default());
        assert!(!chosen.open);
        assert!(chosen.view.is_none());
    }

    /// Runs the toolbar once, with a click delivered at `at`.
    ///
    /// A press and a release at one place, which is what a click is. Nothing
    /// simulates the toolbar itself: the widget under that point is whichever
    /// one really got laid out there.
    fn click_at(context: &egui::Context, at: egui::Pos2) -> Chosen {
        click_on(context, at, Activity::default())
    }

    /// The same, for a window that is in the middle of reading something.
    fn click_on(context: &egui::Context, at: egui::Pos2, activity: Activity<'_>) -> Chosen {
        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        };

        let mut chosen = Chosen::default();
        let mut output = context.run_ui(input, |ui| {
            chosen = toolbar(ui, activity);
        });
        output.textures_delta.clear();
        chosen
    }

    #[test]
    fn the_way_to_open_a_document_is_where_it_can_be_pressed() {
        let context = egui::Context::default();

        // Laid out first in the row, so the first thing the pointer meets is
        // it. A toolbar whose button was never added would report "not
        // pressed" for ever and look, to every other test here, identical to
        // one that works.
        let mut first = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            ui.horizontal(|ui| {
                first = Some(ui.button("Open…").rect);
            });
        });
        output.textures_delta.clear();
        let open = first.expect("the reference row was laid out");
        let centre = open.center();

        assert!(
            click_at(&context, centre).open,
            "nothing at the front of the toolbar opens a document"
        );

        // And the rest of the toolbar is not that button: pressing where the
        // views are must not open a file dialog. Found by walking the row
        // rather than by assuming how far along it they sit, so adding a
        // button to the toolbar cannot quietly turn this into a test that
        // presses empty space.
        let mut found_a_view = false;
        for step in 1..200 {
            let along = egui::Pos2::new(open.right() + step as f32 * 8.0, centre.y);
            let chosen = click_at(&context, along);
            assert!(!chosen.open, "something other than Open opened a document");
            if chosen.view.is_some() {
                found_a_view = true;
            }
        }
        assert!(found_a_view, "no point along the toolbar reached a view");
    }

    #[test]
    fn a_reading_can_be_given_up_on_and_nothing_else_can() {
        let context = egui::Context::default();
        let reading = Activity {
            line: "Opening part.fcad… 40%",
            progress: Some(0.4),
            can_frame_selection: false,
            can_frame_scene: false,
            can_hide: false,
            can_show_all: false,
            can_isolate: false,
            can_undo_visibility: false,
            orthographic: false,
        };

        // Where the Cancel button is: after the views, so the row up to it is
        // laid out the same either way and the difference is this button.
        let mut cancel = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            ui.horizontal(|ui| {
                let _ = ui.button("Open…");
                ui.separator();
                let _ = ui.add_enabled(
                    reading.can_frame_selection,
                    egui::Button::new(format!("Frame selected ({FRAME_KEY})")),
                );
                let _ = ui.add_enabled(
                    reading.can_frame_scene,
                    egui::Button::new(format!("Frame all ({FRAME_ALL_KEY})")),
                );
                let _ = ui.add_enabled(
                    reading.can_hide,
                    egui::Button::new(format!("Hide selected ({HIDE_KEY})")),
                );
                let _ = ui.add_enabled(
                    reading.can_isolate,
                    egui::Button::new(format!("Isolate selected ({ISOLATE_KEY})")),
                );
                let _ = ui.add_enabled(
                    reading.can_show_all,
                    egui::Button::new(format!("Show all ({SHOW_ALL_KEY})")),
                );
                let _ = ui.add_enabled(
                    reading.can_undo_visibility,
                    egui::Button::new("Undo visibility"),
                );
                ui.separator();
                let _ = ui.button(format!(
                    "{} ({PROJECTION_KEY})",
                    if reading.orthographic {
                        "Orthographic"
                    } else {
                        "Perspective"
                    }
                ));
                ui.separator();
                ui.label("View");
                for (_, name, key) in VIEWS {
                    let _ = ui.button(format!("{name} ({key})"));
                }
                ui.separator();
                cancel = Some(ui.button("Cancel").rect);
            });
        });
        output.textures_delta.clear();
        let centre = cancel.expect("the reference row was laid out").center();

        assert!(
            click_on(&context, centre, reading).cancel,
            "nothing there gives up on the reading"
        );

        // And with nothing being read there is nothing to give up on, so that
        // same place must not be a button that quietly does something else.
        let idle = click_on(&context, centre, Activity::default());
        assert!(
            !idle.cancel,
            "a window with nothing to stop offered to stop it"
        );
        assert!(idle.view.is_none() && !idle.open);
    }

    /// The words a viewport uses about one frame, none of which survives it.
    ///
    /// A person reading any of these could write it down and find it means
    /// nothing an hour later, which is the whole reason the inspector deals
    /// in what a document could store.
    const TRANSIENT: &[&str] = &[
        "pick",
        "mesh",
        "face",
        "edge",
        "handle",
        "session",
        "occurrence",
        "instance",
        "index",
        "snapshot",
    ];

    fn body() -> Selected<'static> {
        Selected::Body {
            name: Some("Plate"),
            object: "018f2b7c-0000-7000-8000-000000000001",
        }
    }

    fn a_face() -> Selected<'static> {
        Selected::Face {
            name: Some("Plate"),
            object: "018f2b7c-0000-7000-8000-000000000001",
            names: &[FaceName {
                reference: "018f2b7c-0000-7000-8000-0000000000a1",
                owner: "018f2b7c-0000-7000-8000-000000000001",
                producer_feature: "018f2b7c-0000-7000-8000-000000000002",
                expected_kind: "face",
                role: "Extrusion cap, end",
                rule: "Exactly this one",
            }],
        }
    }

    #[test]
    fn the_inspector_says_what_a_face_is_in_the_document_s_own_terms() {
        let rows = a_face().rows();
        let value = |label: &str| {
            rows.iter()
                .find(|(name, _)| *name == label)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(value("Kind"), Some("Face"));
        assert_eq!(value("Body"), Some("Plate"));
        assert_eq!(value("Role"), Some("Extrusion cap, end"));
        assert_eq!(value("Rule"), Some("Exactly this one"));
        assert_eq!(
            value("Reference"),
            Some("018f2b7c-0000-7000-8000-0000000000a1")
        );
        // Everything shown is something the document stores. Nothing here
        // could be written down and found to mean nothing an hour later.
        let shown = rows
            .iter()
            .map(|(label, value)| format!("{label} {value}"))
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        for word in TRANSIENT {
            if *word == "face" {
                // The word names the kind of thing chosen, which is the one
                // durable use of it: what is refused is a face *number*.
                continue;
            }
            assert!(
                !shown.contains(word),
                "a face inspector said {word}: {shown}"
            );
        }
    }

    fn a_vertex() -> Selected<'static> {
        Selected::Vertex {
            name: Some("Plate"),
            object: "018f2b7c-0000-7000-8000-000000000001",
            names: &[
                VertexName {
                    reference: "018f2b7c-0000-7000-8000-0000000000c1",
                    owner: "018f2b7c-0000-7000-8000-000000000001",
                    producer_feature: "018f2b7c-0000-7000-8000-000000000002",
                    expected_kind: "vertex",
                    role: "Start cap vertex at the joint of profile segments A and B",
                    rule: "Exactly this one",
                },
                VertexName {
                    reference: "018f2b7c-0000-7000-8000-0000000000c2",
                    owner: "018f2b7c-0000-7000-8000-000000000001",
                    producer_feature: "018f2b7c-0000-7000-8000-000000000002",
                    expected_kind: "vertex",
                    role: "End cap vertex at the joint of profile segments A and B",
                    rule: "Exactly this one",
                },
            ],
        }
    }

    #[test]
    fn the_inspector_says_what_a_corner_is_in_the_document_s_own_terms() {
        let rows = a_vertex().rows();
        let value = |label: &str| {
            rows.iter()
                .find(|(name, _)| *name == label)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(value("Kind"), Some("Vertex"));
        assert_eq!(value("Body"), Some("Plate"));
        assert_eq!(
            value("Reference"),
            Some("018f2b7c-0000-7000-8000-0000000000c1")
        );

        // Both stored names, in the order they were given, one sentence each.
        let roles: Vec<&str> = rows
            .iter()
            .filter(|(label, _)| *label == "Role")
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(
            roles,
            [
                "Start cap vertex at the joint of profile segments A and B",
                "End cap vertex at the joint of profile segments A and B",
            ]
        );
        let references: Vec<&str> = rows
            .iter()
            .filter(|(label, _)| *label == "Reference")
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(
            references,
            [
                "018f2b7c-0000-7000-8000-0000000000c1",
                "018f2b7c-0000-7000-8000-0000000000c2",
            ],
            "the inspector reordered or dropped a stored name"
        );

        // Everything shown is something the document stores.
        let shown = rows
            .iter()
            .map(|(label, value)| format!("{label} {value}"))
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        for word in TRANSIENT {
            assert!(
                !shown.contains(word),
                "a corner inspector said {word}: {shown}"
            );
        }
    }

    #[test]
    fn a_corner_in_a_list_of_definitions_is_summarised_as_its_body() {
        assert_eq!(a_vertex().summary(), body().summary());
    }

    #[test]
    fn a_face_with_several_names_shows_all_of_them() {
        let names = [
            FaceName {
                reference: "018f2b7c-0000-7000-8000-0000000000a1",
                owner: "018f2b7c-0000-7000-8000-000000000001",
                producer_feature: "018f2b7c-0000-7000-8000-000000000002",
                expected_kind: "face",
                role: "Extrusion cap, end",
                rule: "Exactly this one",
            },
            FaceName {
                reference: "018f2b7c-0000-7000-8000-0000000000a2",
                owner: "018f2b7c-0000-7000-8000-000000000001",
                producer_feature: "018f2b7c-0000-7000-8000-000000000002",
                expected_kind: "face",
                role: "Extrusion cap, end",
                rule: "Everything derived from 018f2b7c-0000-7000-8000-000000000003",
            },
        ];
        let rows = Selected::Face {
            name: None,
            object: "018f2b7c-0000-7000-8000-000000000001",
            names: &names,
        }
        .rows();

        // Both, in the order they were given. Showing one of two names would
        // be presenting storage order as a decision about which is right.
        let references: Vec<&str> = rows
            .iter()
            .filter(|(label, _)| *label == "Reference")
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(references, vec![names[0].reference, names[1].reference]);
    }

    fn imported() -> Selected<'static> {
        Selected::Imported {
            name: Some("Cube"),
            source_file: Some("03-nested-assembly.step"),
            source: "018f2b7c-0000-7000-8000-0000000000ff",
            definition_key: "step.product_definition#58",
            solids: Some(1),
            geometry_unavailable: None,
        }
    }

    fn unavailable_import() -> Selected<'static> {
        Selected::Imported {
            name: Some("Retained gear"),
            source_file: Some("assembly.step"),
            source: "018f2b7c-0000-7000-8000-0000000000aa",
            definition_key: "step.product_definition#2583",
            solids: Some(1),
            geometry_unavailable: Some(GeometryUnavailable {
                finding_entity: "step.product_definition#2583",
                validation: "the imported definition contains an invalid solid",
                tessellation: "the current tessellator found an incomplete face",
            }),
        }
    }

    #[test]
    fn an_omitted_import_is_visibly_marked_in_the_definitions_list_only() {
        let unavailable = unavailable_import();
        assert!(
            unavailable.summary().contains("geometry unavailable"),
            "the concise list summary hid the omission: {}",
            unavailable.summary()
        );
        assert!(
            !imported().summary().contains("geometry unavailable"),
            "an ordinary imported definition was marked unavailable"
        );
        assert!(
            !body().summary().contains("geometry unavailable"),
            "a native body with no omission was marked unavailable"
        );

        let context = egui::Context::default();
        let shown = list_text(&context, &[unavailable], &[]);
        assert!(
            shown.contains("geometry unavailable"),
            "the actual Definitions panel hid the marker: {shown}"
        );
    }

    #[test]
    fn an_omitted_import_inspector_explains_both_observations_in_portable_terms() {
        let rows = unavailable_import().rows();
        let value = |label: &str| {
            rows.iter()
                .find(|(name, _)| *name == label)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(value("Geometry"), Some("Imported geometry unavailable"));
        assert_eq!(
            value("Finding entity"),
            Some("step.product_definition#2583")
        );
        assert_eq!(
            value("Validation"),
            Some("the imported definition contains an invalid solid")
        );
        assert_eq!(
            value("Tessellation"),
            Some("the current tessellator found an incomplete face")
        );
        assert_eq!(value("Definition"), Some("step.product_definition#2583"));

        let shown = rows
            .iter()
            .map(|(label, value)| format!("{label} {value}"))
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        for forbidden in [
            "/users/",
            "\\users\\",
            "pickid",
            "shapehandle",
            "geometryomission",
            "debug",
            "session",
            "snapshot",
            "0x",
        ] {
            assert!(
                !shown.contains(forbidden),
                "the omission inspector leaked {forbidden:?}: {shown}"
            );
        }
    }

    #[test]
    fn the_inspector_says_what_a_body_is_in_the_document_s_own_terms() {
        let rows = body().rows();
        let value = |label: &str| {
            rows.iter()
                .find(|(name, _)| *name == label)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(value("Kind"), Some("Body"));
        assert_eq!(value("Name"), Some("Plate"));
        assert_eq!(
            value("Object"),
            Some("018f2b7c-0000-7000-8000-000000000001")
        );

        // A body has no file and no counted solids, and inventing rows for
        // them would be answering questions nobody asked of it.
        assert_eq!(value("File"), None);
        assert_eq!(value("Solids"), None);

        // A nameless body is still a body. Nothing is invented for it.
        let rows = Selected::Body {
            name: None,
            object: "018f2b7c-0000-7000-8000-000000000001",
        }
        .rows();
        assert!(rows.iter().all(|(label, _)| *label != "Name"));
    }

    #[test]
    fn the_inspector_says_what_an_imported_definition_is() {
        let rows = imported().rows();
        let value = |label: &str| {
            rows.iter()
                .find(|(name, _)| *name == label)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(value("Kind"), Some("Imported definition"));
        assert_eq!(value("Name"), Some("Cube"));
        assert_eq!(value("File"), Some("03-nested-assembly.step"));
        assert_eq!(
            value("Source"),
            Some("018f2b7c-0000-7000-8000-0000000000ff")
        );
        assert_eq!(value("Definition"), Some("step.product_definition#58"));
        assert_eq!(value("Solids"), Some("1"));

        // The key alone is not the identity, and the panel says so by always
        // showing the file it belongs to beside it: `#58` in another file is
        // another definition entirely.
        assert!(rows.iter().any(|(label, _)| *label == "Source"));
    }

    #[test]
    fn the_inspector_never_puts_a_transient_word_on_screen() {
        for selected in [body(), imported()] {
            for (label, value) in selected.rows() {
                let text = format!("{label} {value}").to_lowercase();
                for word in TRANSIENT {
                    assert!(
                        !text.contains(word),
                        "the inspector would show {word:?} in {label:?}: {value:?}"
                    );
                }
                assert!(
                    !value.contains('/') && !value.contains('\\'),
                    "the inspector would show a path in {label:?}: {value:?}"
                );
            }
        }
    }

    #[test]
    fn an_empty_selection_says_so_rather_than_showing_nothing() {
        let context = egui::Context::default();

        let mut empty = context.run_ui(egui::RawInput::default(), |ui| {
            selection_inspector(ui, None);
        });
        let mut chosen = context.run_ui(egui::RawInput::default(), |ui| {
            selection_inspector(ui, Some(imported()));
        });
        let mut nothing_at_all = context.run_ui(egui::RawInput::default(), |_| {});

        let vertices = |output: &egui::FullOutput| {
            context
                .clone()
                .tessellate(output.shapes.clone(), 1.0)
                .into_iter()
                .map(|primitive| match primitive.primitive {
                    egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                    egui::epaint::Primitive::Callback(_) => 0,
                })
                .sum::<usize>()
        };

        // Something is drawn either way, and more of it when there is more to
        // say. An interface that went blank on an empty selection would be
        // indistinguishable from one that had stopped working.
        assert!(vertices(&empty) > vertices(&nothing_at_all));
        assert!(vertices(&chosen) > vertices(&empty));

        empty.textures_delta.clear();
        chosen.textures_delta.clear();
        nothing_at_all.textures_delta.clear();
    }

    #[test]
    fn hiding_and_showing_are_offered_exactly_when_they_would_do_something() {
        let context = egui::Context::default();
        let state = |can_hide, can_show_all| Activity {
            line: "part.fcad",
            progress: None,
            can_frame_selection: false,
            can_frame_scene: false,
            can_hide,
            can_show_all,
            can_isolate: false,
            can_undo_visibility: false,
            orthographic: false,
        };

        // Found by pressing along the real toolbar rather than by rebuilding
        // its layout here: a gate that assumed where a button sits would pass
        // while pressing empty space.
        let row = 12.0;
        let mut hide = None;
        let mut show = None;
        for step in 0..200 {
            let at = egui::Pos2::new(step as f32 * 8.0, row);
            let chosen = click_on(&context, at, state(true, true));
            if chosen.hide && hide.is_none() {
                hide = Some(at);
            }
            if chosen.show_all && show.is_none() {
                show = Some(at);
            }
        }
        let hide = hide.expect("the toolbar offers no way to hide what is chosen");
        let show = show.expect("the toolbar offers no way to show everything");
        assert_ne!(hide, show, "one button asked for both things");

        // A frame with the pointer away from the toolbar before each press.
        // egui decides a click from the frame before as well as this one, so
        // consecutive probes at different places would otherwise report what
        // the previous probe left behind – and a context that has never laid
        // the row out lays it out differently the first time.
        let press = |at, activity| {
            let _ = click_on(&context, egui::Pos2::new(2000.0, 2000.0), activity);
            click_on(&context, at, activity)
        };

        // Available exactly when there is something to do: a button that
        // reports a press with nothing to act on is a button that lies about
        // what the window can do.
        assert!(press(hide, state(true, false)).hide);
        assert!(!press(hide, state(false, false)).hide);
        assert!(press(show, state(false, true)).show_all);
        assert!(!press(show, state(false, false)).show_all);

        // And neither is the other.
        let pressed_hide = press(hide, state(true, true));
        assert!(pressed_hide.hide && !pressed_hide.show_all);
        let pressed_show = press(show, state(true, true));
        assert!(pressed_show.show_all && !pressed_show.hide);
        // Nor is either of them a view or a way to open a document.
        assert!(pressed_hide.view.is_none() && !pressed_hide.open);
        assert!(pressed_show.view.is_none() && !pressed_show.open);
    }

    #[test]
    fn a_hidden_row_is_shown_as_hidden_and_answers_nothing() {
        let context = egui::Context::default();
        let definitions = [body(), imported()];
        let one = a_pick(1);
        let two = a_pick(2);
        let drawn = [RowVisibility::Hide(one), RowVisibility::Hide(two)];
        let missing = [RowVisibility::Hide(one), RowVisibility::Show(two)];

        // Everywhere in the list, in both states. Pressing anywhere rather
        // than at a computed point: what a hidden row must not do, it must not
        // do at any pixel of itself, and a gate that pressed one place could
        // pass by pressing the wrong one.
        let reaches = |offers: &[RowVisibility], row: usize| {
            let mut pressed = false;
            let mut hovered = false;
            for step_y in 0..30 {
                for step_x in 0..40 {
                    let at = egui::Pos2::new(step_x as f32 * 8.0, step_y as f32 * 5.0);
                    let _ = press_list(
                        &context,
                        egui::Pos2::new(4000.0, 4000.0),
                        &definitions,
                        None,
                        offers,
                    );
                    let rows = press_list(&context, at, &definitions, None, offers);
                    pressed |= rows.pressed == Some(row);
                    hovered |= rows.hovered == Some(row);
                }
            }
            (pressed, hovered)
        };

        // Drawn, the row answers; hidden, it answers nothing anywhere. The
        // first half is what makes the second half mean anything.
        assert_eq!(
            reaches(&drawn, 1),
            (true, true),
            "a drawn row cannot be chosen or pointed at"
        );
        assert_eq!(
            reaches(&missing, 1),
            (false, false),
            "a hidden row chose or pointed at invisible geometry"
        );
        // And the row beside it still works while it is hidden, so this is
        // about being hidden and not about the list having stopped.
        assert_eq!(reaches(&missing, 0), (true, true));

        // Still in the list: a row that vanished when it was hidden would be a
        // row with no way back to it.
        assert_eq!(
            rows_of_with(&context, &definitions, &missing).len(),
            2,
            "a hidden definition left the list"
        );

        // And it says so: the same row reads differently when it is hidden.
        let plain = list_text(&context, &definitions, &drawn);
        let marked = list_text(&context, &definitions, &missing);
        assert_ne!(plain, marked, "nothing on screen says the row is hidden");
        assert!(marked.contains("hidden"));
    }

    /// A pick of a picture with `count` definitions, naming the last of them.
    ///
    /// The panel never reads one; it carries them. What matters here is that
    /// two rows carry two different ones.
    fn a_pick(count: usize) -> PickId {
        let mut builder = ferritecad_viewport::SnapshotBuilder::new();
        let mesh = ferritecad_kernel::Mesh {
            topological_vertices: None,
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![ferritecad_kernel::MeshFaceRange {
                face: ferritecad_kernel::SubShapeHandle::new(
                    ferritecad_kernel::ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1),
                    ferritecad_kernel::SubShapeKind::Face,
                    0,
                ),
                first_index: 0,
                index_count: 3,
            }],
            edges: None,
        };
        for _ in 0..count {
            let definition = builder.add_mesh(&mesh).expect("packs");
            builder
                .place(
                    definition,
                    None,
                    &ferritecad_types::Transform::IDENTITY,
                    [1.0, 1.0, 1.0],
                )
                .expect("places");
        }
        builder
            .build()
            .pick_of(count - 1)
            .expect("the picture has that row")
    }

    /// Every glyph the list draws, as one string.
    fn list_text(
        context: &egui::Context,
        definitions: &[Selected<'_>],
        offers: &[RowVisibility],
    ) -> String {
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            let _ = definitions_panel(ui, definitions, None, offers);
        });
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        output.textures_delta.clear();
        text
    }

    #[test]
    fn isolating_is_offered_exactly_when_something_else_is_still_drawn() {
        let context = egui::Context::default();
        let state = |can_isolate| Activity {
            line: "part.fcad",
            progress: None,
            can_frame_selection: false,
            can_frame_scene: false,
            can_hide: true,
            can_show_all: true,
            can_isolate,
            can_undo_visibility: false,
            orthographic: false,
        };

        // Found by pressing along the real toolbar rather than by rebuilding
        // its layout here.
        let mut isolate = None;
        for step in 0..200 {
            let at = egui::Pos2::new(step as f32 * 8.0, 12.0);
            if click_on(&context, at, state(true)).isolate {
                isolate = Some(at);
                break;
            }
        }
        let isolate = isolate.expect("the toolbar offers no way to isolate what is chosen");

        // A frame with the pointer away before each press: egui decides a
        // click from the frame before as well as this one.
        let press = |activity| {
            let _ = click_on(&context, egui::Pos2::new(2000.0, 2000.0), activity);
            click_on(&context, isolate, activity)
        };

        let pressed = press(state(true));
        assert!(pressed.isolate);
        // One press, one request: the button beside it must not fire too.
        assert!(!pressed.hide && !pressed.show_all);
        assert!(pressed.view.is_none() && !pressed.open && !pressed.frame);

        assert!(
            !press(state(false)).isolate,
            "a button with nothing to isolate reported a press"
        );
    }

    #[test]
    fn the_isolate_button_prints_the_key_that_reaches_it() {
        // The panel prints `ISOLATE_KEY`, and whatever binds the keyboard
        // reads the same constant. A shortcut that drifts from its label is a
        // shortcut nobody can trust.
        assert!(!ISOLATE_KEY.is_empty());
        assert_ne!(ISOLATE_KEY, HIDE_KEY);
        assert_ne!(ISOLATE_KEY, SHOW_ALL_KEY);
        assert_ne!(ISOLATE_KEY, FRAME_KEY);
        assert_ne!(ISOLATE_KEY, FRAME_ALL_KEY);
        assert!(
            VIEWS.iter().all(|(_, _, key)| *key != ISOLATE_KEY),
            "the isolate key is also a view key"
        );

        let context = egui::Context::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            let _ = toolbar(
                ui,
                Activity {
                    line: "part.fcad",
                    progress: None,
                    can_frame_selection: false,
                    can_frame_scene: false,
                    can_hide: false,
                    can_show_all: false,
                    can_isolate: true,
                    can_undo_visibility: false,
                    orthographic: false,
                },
            );
        });
        let printed = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        output.textures_delta.clear();
        assert!(
            printed.contains(&format!("Isolate selected ({ISOLATE_KEY})")),
            "the button does not print the key that reaches it: {printed}"
        );
    }

    #[test]
    fn a_row_offers_exactly_one_way_to_change_what_is_drawn() {
        let context = egui::Context::default();
        let definitions = [body(), imported()];
        let one = a_pick(1);
        let two = a_pick(2);

        // One row drawn and one hidden, so both controls are on screen at
        // once and each can be shown to ask for its own row.
        let offers = [RowVisibility::Hide(one), RowVisibility::Show(two)];
        let mut asked: Vec<(egui::Pos2, RowVisibility)> = Vec::new();
        for step_y in 0..30 {
            for step_x in 0..40 {
                let at = egui::Pos2::new(step_x as f32 * 8.0, step_y as f32 * 5.0);
                let _ = press_list(
                    &context,
                    egui::Pos2::new(4000.0, 4000.0),
                    &definitions,
                    None,
                    &offers,
                );
                let rows = press_list(&context, at, &definitions, None, &offers);
                if let Some(asked_for) = rows.visibility {
                    // Pressing a control is not pressing the row, and not
                    // pointing at it either.
                    assert_eq!(rows.pressed, None, "a control also chose its row");
                    assert_eq!(rows.hovered, None, "a control also pointed at its row");
                    asked.push((at, asked_for));
                }
            }
        }

        // Both controls are reachable, each asks for its own definition, and
        // no press asks for both.
        assert!(
            asked
                .iter()
                .any(|(_, what)| *what == RowVisibility::Hide(one)),
            "the drawn row offers no way to take it off screen"
        );
        assert!(
            asked
                .iter()
                .any(|(_, what)| *what == RowVisibility::Show(two)),
            "the hidden row offers no way back"
        );
        for (at, what) in &asked {
            assert!(
                matches!(what, RowVisibility::Hide(pick) if *pick == one)
                    || matches!(what, RowVisibility::Show(pick) if *pick == two),
                "the control at {at:?} asked for the wrong definition: {what:?}"
            );
        }

        // A row with nothing to offer offers nothing anywhere: a definition
        // that draws nothing wherever it is has no way in or out.
        let neither = [RowVisibility::Neither, RowVisibility::Neither];
        for step_y in 0..30 {
            for step_x in 0..40 {
                let at = egui::Pos2::new(step_x as f32 * 8.0, step_y as f32 * 5.0);
                let _ = press_list(
                    &context,
                    egui::Pos2::new(4000.0, 4000.0),
                    &definitions,
                    None,
                    &neither,
                );
                assert_eq!(
                    press_list(&context, at, &definitions, None, &neither).visibility,
                    None,
                    "a row with nothing to offer offered something at {at:?}"
                );
            }
        }
    }

    #[test]
    fn taking_a_change_back_is_offered_only_when_there_is_one() {
        let context = egui::Context::default();
        let state = |can_undo_visibility| Activity {
            line: "part.fcad",
            progress: None,
            can_frame_selection: false,
            can_frame_scene: false,
            can_hide: true,
            can_show_all: true,
            can_isolate: true,
            can_undo_visibility,
            orthographic: false,
        };

        // Found by pressing along the real toolbar rather than by rebuilding
        // its layout here.
        let mut undo = None;
        for step in 0..200 {
            let at = egui::Pos2::new(step as f32 * 8.0, 12.0);
            if click_on(&context, at, state(true)).undo_visibility {
                undo = Some(at);
                break;
            }
        }
        let undo = undo.expect("the toolbar offers no way to take a change back");

        let press = |activity| {
            let _ = click_on(&context, egui::Pos2::new(2000.0, 2000.0), activity);
            click_on(&context, undo, activity)
        };

        // One press, one request, and nothing else asked for.
        let pressed = press(state(true));
        assert!(pressed.undo_visibility);
        assert!(!pressed.hide && !pressed.show_all && !pressed.isolate);
        assert!(pressed.view.is_none() && !pressed.open && !pressed.frame);

        // With nothing to take back, the same place reports nothing: a button
        // that answered anyway would be claiming a change nobody can see.
        assert!(
            !press(state(false)).undo_visibility,
            "a button with nothing to take back reported a press"
        );
    }

    #[test]
    fn the_projection_control_says_which_one_is_in_use() {
        let context = egui::Context::default();
        let state = |orthographic| Activity {
            line: "part.fcad",
            progress: None,
            can_frame_selection: false,
            can_frame_scene: false,
            can_hide: false,
            can_show_all: false,
            can_isolate: false,
            can_undo_visibility: false,
            orthographic,
        };

        // What the toolbar draws in each state. A control that only said
        // "projection" would leave a person to work out which one they are
        // looking at from the picture, which is the hard part.
        let printed = |orthographic| {
            let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                let _ = toolbar(ui, state(orthographic));
            });
            let text = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            output.textures_delta.clear();
            text
        };

        let seen = printed(false);
        assert!(
            seen.contains(&format!("Perspective ({PROJECTION_KEY})")),
            "the toolbar does not say it is drawing as an eye sees: {seen}"
        );
        assert!(!seen.contains("Orthographic"));
        let drawn = printed(true);
        assert!(
            drawn.contains(&format!("Orthographic ({PROJECTION_KEY})")),
            "the toolbar does not say it is drawing as a drawing shows: {drawn}"
        );
        assert!(!drawn.contains("Perspective"));

        // And the key it prints is not one another action already answers.
        assert_ne!(PROJECTION_KEY, FRAME_KEY);
        assert_ne!(PROJECTION_KEY, FRAME_ALL_KEY);
        assert_ne!(PROJECTION_KEY, HIDE_KEY);
        assert_ne!(PROJECTION_KEY, SHOW_ALL_KEY);
        assert_ne!(PROJECTION_KEY, ISOLATE_KEY);
        assert!(VIEWS.iter().all(|(_, _, key)| *key != PROJECTION_KEY));

        // Pressing it asks for the projection and for nothing else.
        let mut at = None;
        for step in 0..200 {
            let point = egui::Pos2::new(step as f32 * 8.0, 12.0);
            if click_on(&context, point, state(false)).projection {
                at = Some(point);
                break;
            }
        }
        let at = at.expect("the toolbar offers no way to change projection");
        let _ = click_on(&context, egui::Pos2::new(2000.0, 2000.0), state(false));
        let pressed = click_on(&context, at, state(false));
        assert!(pressed.projection);
        assert!(!pressed.hide && !pressed.show_all && !pressed.isolate);
        assert!(!pressed.undo_visibility && !pressed.frame && !pressed.frame_all);
        assert!(pressed.view.is_none() && !pressed.open);
    }

    /// Runs the list once, with a click delivered at `at`.
    fn press_list(
        context: &egui::Context,
        at: egui::Pos2,
        definitions: &[Selected<'_>],
        chosen: Option<usize>,
        offers: &[RowVisibility],
    ) -> Rows {
        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        };

        let mut rows = Rows::default();
        let mut output = context.run_ui(input, |ui| {
            rows = definitions_panel(ui, definitions, chosen, offers);
        });
        output.textures_delta.clear();
        rows
    }

    /// Where the panel lays each of its rows out.
    ///
    /// The same structure the panel builds, in a `Ui` that has had nothing
    /// else put in it – which is how the panel is drawn when it is pressed
    /// below. Measuring a second copy underneath would give the coordinates of
    /// the copy.
    fn rows_of(context: &egui::Context, definitions: &[Selected<'_>]) -> Vec<egui::Rect> {
        rows_of_with(context, definitions, &[])
    }

    /// The same, for a list in which some definitions are hidden.
    fn rows_of_with(
        context: &egui::Context,
        definitions: &[Selected<'_>],
        offers: &[RowVisibility],
    ) -> Vec<egui::Rect> {
        let mut rects = Vec::new();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .show(ui, |ui| {
                    for (row, definition) in definitions.iter().enumerate() {
                        let offer = offers.get(row).copied().unwrap_or(RowVisibility::Neither);
                        let is_hidden = offer.is_hidden();
                        let summary = if is_hidden {
                            format!("{} · hidden", definition.summary())
                        } else {
                            definition.summary()
                        };
                        // The same shape the panel lays out, so the places
                        // measured here are the places pressed there.
                        rects.push(
                            ui.horizontal(|ui| {
                                ui.add_enabled(!is_hidden, egui::Button::selectable(false, summary))
                                    .rect
                            })
                            .inner,
                        );
                    }
                });
        });
        output.textures_delta.clear();
        rects
    }

    #[test]
    fn a_definition_nothing_is_pointing_at_can_still_be_chosen() {
        let context = egui::Context::default();
        // Two definitions, and the second one is the one a click could not
        // reach: hidden behind the first, too small to hit, or off screen.
        // A list does not care where anything is drawn.
        let definitions = [body(), imported()];

        let rows = rows_of(&context, &definitions);
        assert_eq!(rows.len(), 2, "the list did not lay out one row each");

        let pressed = press_list(&context, rows[1].center(), &definitions, None, &[]);
        assert_eq!(
            pressed.pressed,
            Some(1),
            "the second definition could not be chosen from the list"
        );

        // And pressing nothing chooses nothing: a list that reported a press
        // every frame would reselect for as long as the window was open.
        let quiet = context.run_ui(egui::RawInput::default(), |ui| {
            assert_eq!(
                definitions_panel(ui, &definitions, Some(1), &[]).pressed,
                None
            );
        });
        let mut quiet = quiet;
        quiet.textures_delta.clear();
    }

    #[test]
    fn pointing_at_a_row_asks_about_it_without_choosing_it() {
        let context = egui::Context::default();
        let definitions = [body(), imported()];
        let rows = rows_of(&context, &definitions);

        // Moving over a row, with no button involved.
        let over = |at: egui::Pos2| {
            let mut asked = Rows::default();
            let mut output = context.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::PointerMoved(at)],
                    ..Default::default()
                },
                |ui| {
                    asked = definitions_panel(ui, &definitions, None, &[]);
                },
            );
            output.textures_delta.clear();
            asked
        };

        let asked = over(rows[1].center());
        assert_eq!(asked.hovered, Some(1), "pointing at a row asked nothing");
        assert_eq!(
            asked.pressed, None,
            "pointing at a row chose it, so nothing could be looked at without \
             being selected"
        );

        // Somewhere else in the list is a different question, and off the list
        // is no question at all.
        assert_eq!(over(rows[0].center()).hovered, Some(0));
        assert_eq!(
            over(egui::Pos2::new(
                rows[0].center().x,
                rows[0].center().y + 400.0
            ))
            .hovered,
            None
        );
    }

    #[test]
    fn two_definitions_with_one_name_are_two_rows() {
        let context = egui::Context::default();
        // Legal, and the case a name-keyed list would collapse: same name,
        // different portable identity.
        let definitions = [
            Selected::Body {
                name: Some("Plate"),
                object: "018f2b7c-0000-7000-8000-000000000001",
            },
            Selected::Body {
                name: Some("Plate"),
                object: "018f2b7c-0000-7000-8000-000000000002",
            },
        ];

        let rows = rows_of(&context, &definitions);
        assert_eq!(rows.len(), 2, "two definitions were shown as one row");
        assert_ne!(
            definitions[0].summary(),
            definitions[1].summary(),
            "two rows read alike, so nothing on screen tells them apart"
        );

        // Each is chosen on its own.
        assert_eq!(
            press_list(&context, rows[0].center(), &definitions, None, &[]).pressed,
            Some(0)
        );
        assert_eq!(
            press_list(&context, rows[1].center(), &definitions, None, &[]).pressed,
            Some(1)
        );
    }

    #[test]
    fn the_same_imported_name_key_and_file_still_show_which_source_is_which() {
        // Two unrelated files may share a basename and use the same ordinary
        // PRODUCT_DEFINITION number. Their source identities are the only
        // portable fact that distinguishes them, so leaving the source out of
        // the summary makes two different rows read exactly alike.
        let first = Selected::Imported {
            name: Some("Bracket"),
            source_file: Some("part.step"),
            source: "018f2b7c-0000-7000-8000-000000000001",
            definition_key: "step.product_definition#5",
            solids: Some(1),
            geometry_unavailable: None,
        };
        let second = Selected::Imported {
            name: Some("Bracket"),
            source_file: Some("part.step"),
            source: "018f2b7c-0000-7000-8000-000000000002",
            definition_key: "step.product_definition#5",
            solids: Some(1),
            geometry_unavailable: None,
        };

        assert_ne!(
            first.summary(),
            second.summary(),
            "two imported definitions with different identities read alike"
        );
    }

    #[test]
    fn an_empty_picture_says_so_and_offers_nothing_to_choose() {
        let context = egui::Context::default();
        let mut rows = Rows {
            pressed: Some(7),
            hovered: Some(7),
            visibility: None,
        };
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            rows = definitions_panel(ui, &[], None, &[]);
        });
        output.textures_delta.clear();
        assert_eq!(rows, Rows::default(), "an empty list invented a choice");

        // And it says something rather than drawing an empty strip.
        let mut empty = context.run_ui(egui::RawInput::default(), |_| {});
        let vertices = |output: &egui::FullOutput| {
            context
                .clone()
                .tessellate(output.shapes.clone(), 1.0)
                .into_iter()
                .map(|primitive| match primitive.primitive {
                    egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                    egui::epaint::Primitive::Callback(_) => 0,
                })
                .sum::<usize>()
        };
        let mut said = context.run_ui(egui::RawInput::default(), |ui| {
            let _ = definitions_panel(ui, &[], None, &[]);
        });
        assert!(vertices(&said) > vertices(&empty));
        said.textures_delta.clear();
        empty.textures_delta.clear();
    }

    #[test]
    fn a_row_never_names_anything_that_dies_with_the_picture() {
        for definition in [body(), imported()] {
            let text = definition.summary().to_lowercase();
            for word in TRANSIENT {
                assert!(
                    !text.contains(word),
                    "a row would show {word:?}: {}",
                    definition.summary()
                );
            }
            assert!(
                !definition.summary().contains('/') && !definition.summary().contains('\\'),
                "a row would show a path: {}",
                definition.summary()
            );
        }
    }

    #[test]
    fn showing_what_is_chosen_is_offered_only_when_there_is_somewhere_to_go() {
        let context = egui::Context::default();
        let can = |can_frame_selection| Activity {
            line: "part.fcad",
            progress: None,
            can_frame_selection,
            can_frame_scene: false,
            can_hide: false,
            can_show_all: false,
            can_isolate: false,
            can_undo_visibility: false,
            orthographic: false,
        };

        // Where the button is, laid out exactly as the toolbar lays it out.
        let mut frame = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            ui.horizontal(|ui| {
                let _ = ui.button("Open…");
                ui.separator();
                frame = Some(
                    ui.add_enabled(
                        true,
                        egui::Button::new(format!("Frame selected ({FRAME_KEY})")),
                    )
                    .rect,
                );
            });
        });
        output.textures_delta.clear();
        let centre = frame.expect("the reference row was laid out").center();

        assert!(
            click_on(&context, centre, can(true)).frame,
            "nothing there asks to see what is chosen"
        );

        // With nothing to show, the same press asks for nothing: an action
        // that cannot be carried out must not report that it was.
        assert!(
            !click_on(&context, centre, can(false)).frame,
            "an unavailable action reported that it happened"
        );

        // And an untouched toolbar asks for nothing either way.
        let mut quiet = Chosen::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            quiet = toolbar(ui, can(true));
        });
        output.textures_delta.clear();
        assert!(!quiet.frame);
    }

    #[test]
    fn showing_everything_is_offered_only_when_there_is_anything_to_show() {
        let context = egui::Context::default();
        let with = |can_frame_scene| Activity {
            line: "part.fcad",
            progress: None,
            can_frame_selection: false,
            can_frame_scene,
            can_hide: false,
            can_show_all: false,
            can_isolate: false,
            can_undo_visibility: false,
            orthographic: false,
        };

        // Where the button is, laid out exactly as the toolbar lays it out.
        let mut all = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            ui.horizontal(|ui| {
                let _ = ui.button("Open…");
                ui.separator();
                let _ = ui.add_enabled(
                    false,
                    egui::Button::new(format!("Frame selected ({FRAME_KEY})")),
                );
                all = Some(
                    ui.add_enabled(
                        true,
                        egui::Button::new(format!("Frame all ({FRAME_ALL_KEY})")),
                    )
                    .rect,
                );
            });
        });
        output.textures_delta.clear();
        let centre = all.expect("the reference row was laid out").center();

        assert!(
            click_on(&context, centre, with(true)).frame_all,
            "nothing there asks to see the whole model"
        );

        // A picture with nothing in it has nowhere to point a camera, and
        // pressing where the button is must not pretend otherwise.
        assert!(
            !click_on(&context, centre, with(false)).frame_all,
            "an empty picture offered somewhere to go"
        );

        // Showing everything is not showing what is chosen: one press is one
        // request, and not both.
        assert!(!click_on(&context, centre, with(true)).frame);

        let mut quiet = Chosen::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            quiet = toolbar(ui, with(true));
        });
        output.textures_delta.clear();
        assert!(!quiet.frame_all, "an untouched toolbar asked to reframe");
    }

    #[test]
    fn the_key_printed_on_the_button_is_the_one_this_crate_names() {
        // The panel prints `FRAME_KEY` and whatever binds the keyboard reads
        // the same constant, so a shortcut cannot end up printed on a button
        // that does something else.
        assert!(!FRAME_KEY.is_empty());
        assert!(!FRAME_ALL_KEY.is_empty());
        assert!(
            VIEWS.iter().all(|(_, _, key)| *key != FRAME_KEY),
            "the framing key is also printed on a view button"
        );
        assert!(
            VIEWS.iter().all(|(_, _, key)| *key != FRAME_ALL_KEY),
            "the whole-model key is also printed on a view button"
        );
        assert_ne!(
            FRAME_KEY, FRAME_ALL_KEY,
            "one key would mean two things depending on what is chosen"
        );
    }

    #[test]
    fn every_standard_view_is_offered_and_named_once() {
        // A panel that quietly dropped one would leave a direction reachable
        // only by a key nobody was told about.
        let mut seen: Vec<StandardView> = Vec::new();
        for (view, name, key) in VIEWS {
            assert!(!name.is_empty(), "{view:?} has no name");
            assert!(!key.is_empty(), "{view:?} has no shortcut");
            assert!(!seen.contains(view), "{view:?} is offered twice");
            seen.push(*view);
        }
        assert_eq!(seen.len(), 7);

        let mut keys: Vec<&str> = VIEWS.iter().map(|(_, _, key)| *key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), VIEWS.len(), "two views share a shortcut");
    }

    /// Two identifiers of the shape a document actually stores.
    ///
    /// Real UUIDv7 text rather than short stand-ins: what is being checked
    /// includes that a whole identifier reaches the screen, and a three-letter
    /// stand-in would be shown whole by a panel that truncates.
    const SKETCH_ID: &str = "01930f2c-1a2b-7c3d-8e4f-0a1b2c3d4e5f";
    const FIRST_REDUNDANT: &str = "01930f2c-1a2b-7c3d-8e4f-111111111111";
    const SECOND_REDUNDANT: &str = "01930f2c-1a2b-7c3d-8e4f-222222222222";

    /// What each of those two constraints says, already written by the caller.
    ///
    /// Finished sentences rather than fragments, because this panel neither
    /// composes them nor inspects them: what is checked here is that a whole
    /// one reaches a person, beside the whole identifier it belongs to.
    const FIRST_SAYS: &str = "Coincident: 01930f2c-1a2b-7c3d-8e4f-333333333333.end and \
                              01930f2c-1a2b-7c3d-8e4f-444444444444.start are the same point";
    const SECOND_SAYS: &str = "Distance: 01930f2c-1a2b-7c3d-8e4f-333333333333.start and \
                               01930f2c-1a2b-7c3d-8e4f-333333333333.end are 60 mm apart";

    /// Two repeated constraints, each with its identifier and its sentence.
    fn repeats() -> [RedundantExplanation<'static>; 2] {
        [
            RedundantExplanation {
                identifier: FIRST_REDUNDANT,
                says: FIRST_SAYS,
            },
            RedundantExplanation {
                identifier: SECOND_REDUNDANT,
                says: SECOND_SAYS,
            },
        ]
    }

    /// Every word this panel actually drew, in the order it drew them.
    ///
    /// Read out of the shapes egui produced rather than out of the values that
    /// went in: what is being checked is what a person reads, and a panel that
    /// laid out nothing would satisfy any assertion made against its input.
    fn words_drawn(sketches: &[SolvedSketch<'_>]) -> Vec<String> {
        let context = egui::Context::default();
        // A frame first, so the fonts are loaded: without them there is
        // nothing to lay a galley out with and every line comes back empty.
        let mut warm = context.run_ui(egui::RawInput::default(), |_| {});
        warm.textures_delta.clear();

        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            sketch_solves_panel(ui, sketches);
        });
        output.textures_delta.clear();
        let mut words = Vec::new();
        for clipped in output.shapes {
            collect_text(&clipped.shape, &mut words);
        }
        words
    }

    /// What a section painted, counted by kind of mark.
    ///
    /// The one signal that tells a read-only section from a section with
    /// something pressable in it. A label paints a galley and nothing else; a
    /// button, a checkbox, a slider or a chosen row paints itself a background
    /// or a frame first, and that is a rectangle. Asking egui whether a click
    /// "reached a widget" does not tell them apart: a plain label is already
    /// click-sensing, because that is how a person selects text in one.
    fn marks(
        shapes: &[egui::epaint::ClippedShape],
    ) -> std::collections::BTreeMap<&'static str, usize> {
        fn kind(shape: &egui::Shape, into: &mut std::collections::BTreeMap<&'static str, usize>) {
            let name = match shape {
                egui::Shape::Text(_) => "text",
                egui::Shape::LineSegment { .. } => "line",
                egui::Shape::Noop => return,
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        kind(shape, into);
                    }
                    return;
                }
                // Everything a widget paints for itself: a background, a
                // frame, a tick, a handle.
                _ => "furniture",
            };
            *into.entry(name).or_default() += 1;
        }
        let mut counted = std::collections::BTreeMap::new();
        for clipped in shapes {
            kind(&clipped.shape, &mut counted);
        }
        counted
    }

    /// Every galley in one shape, however deeply it is nested.
    fn collect_text(shape: &egui::Shape, into: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => into.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, into);
                }
            }
            _ => {}
        }
    }

    /// Everything the panel drew, as one string to search.
    fn page(sketches: &[SolvedSketch<'_>]) -> String {
        words_drawn(sketches).join("\n")
    }

    /// A sketch that cannot move, with nothing repeated in it.
    fn settled() -> SolvedSketch<'static> {
        SolvedSketch {
            name: Some("Profile"),
            object: SKETCH_ID,
            degrees_of_freedom: 0,
            redundant: &[],
        }
    }

    #[test]
    fn a_section_with_nothing_to_report_says_so() {
        let page = page(&[]);
        assert!(
            page.contains("Sketch solves"),
            "the section did not name itself:\n{page}"
        );
        assert!(
            page.contains("No solved constrained sketches"),
            "a document with no constrained sketch left the section blank, which reads as a \
             section that failed:\n{page}"
        );
    }

    #[test]
    fn a_sketch_that_cannot_move_is_called_fully_constrained() {
        let page = page(&[settled()]);
        assert!(
            page.contains("Fully constrained"),
            "a sketch with no freedom left was not called fully constrained:\n{page}"
        );
        assert!(
            !page.contains("Under-constrained"),
            "a sketch with no freedom left was called under-constrained:\n{page}"
        );
        assert!(
            page.contains("Profile"),
            "the sketch was not named:\n{page}"
        );
        assert!(
            page.contains(SKETCH_ID),
            "the whole identifier the document stores did not reach the screen:\n{page}"
        );
    }

    #[test]
    fn a_sketch_with_freedom_left_says_how_much_of_it() {
        let page = page(&[SolvedSketch {
            name: Some("Loose"),
            object: SKETCH_ID,
            degrees_of_freedom: 2,
            redundant: &[],
        }]);
        assert!(
            page.contains("Under-constrained"),
            "a sketch with freedom left was not called under-constrained:\n{page}"
        );
        assert!(
            !page.contains("Fully constrained"),
            "a sketch with freedom left was called fully constrained:\n{page}"
        );
        // The number itself, and the two numbers it is most easily confused
        // with: a panel showing "0" or "1" here would look right in a
        // screenshot and be wrong about the drawing.
        let rows = words_drawn(&[SolvedSketch {
            name: Some("Loose"),
            object: SKETCH_ID,
            degrees_of_freedom: 2,
            redundant: &[],
        }]);
        assert!(
            rows.iter().any(|word| word == "2"),
            "the exact number of degrees of freedom was not shown: {rows:?}"
        );
        assert!(
            page.contains("Degrees of freedom"),
            "the number was shown without saying what it counts:\n{page}"
        );
    }

    #[test]
    fn a_sketch_the_document_never_named_is_still_shown() {
        let page = page(&[SolvedSketch {
            name: None,
            object: SKETCH_ID,
            degrees_of_freedom: 1,
            redundant: &[],
        }]);
        assert!(
            page.contains("Unnamed sketch"),
            "a sketch nobody named was dropped from the section, or shown with a blank line \
             where its name goes:\n{page}"
        );
        assert!(
            page.contains(SKETCH_ID),
            "the unnamed sketch was shown without the one thing that identifies it:\n{page}"
        );
        assert!(
            page.contains("Degrees of freedom"),
            "an unnamed sketch was shown without what its solve found out:\n{page}"
        );
    }

    #[test]
    fn a_sketch_that_repeats_nothing_says_none_rather_than_leaving_a_gap() {
        let page = page(&[settled()]);
        assert!(
            page.contains("Redundant"),
            "the section said nothing at all about repeated constraints:\n{page}"
        );
        assert!(
            page.contains("None"),
            "a sketch that repeats nothing left the line blank, which reads the same as a list \
             that failed to arrive:\n{page}"
        );
    }

    #[test]
    fn every_repeated_constraint_is_named_whole_and_in_the_documents_order() {
        let redundant = repeats();
        let rows = words_drawn(&[SolvedSketch {
            name: Some("Profile"),
            object: SKETCH_ID,
            degrees_of_freedom: 0,
            redundant: &redundant,
        }]);
        let page = rows.join("\n");
        let first = page.find(FIRST_REDUNDANT);
        let second = page.find(SECOND_REDUNDANT);
        assert!(
            first.is_some(),
            "the first repeated constraint was not named:\n{page}"
        );
        assert!(
            second.is_some(),
            "the second repeated constraint was not named:\n{page}"
        );
        assert!(
            first < second,
            "the repeated constraints were reordered, so the list is no longer the document's:\
             \n{page}"
        );
        assert!(
            !page.contains("None"),
            "a sketch that repeats two constraints also said it repeats none:\n{page}"
        );
    }

    #[test]
    fn every_repeated_constraint_carries_its_explanation_beside_its_identifier() {
        // The defect this closes was a list of identifiers and nothing else.
        // Both halves have to be on screen, and each pair has to belong to the
        // constraint above it: an identifier under the wrong sentence is worse
        // than no sentence at all.
        let redundant = repeats();
        let drawn = words_drawn(&[SolvedSketch {
            name: Some("Profile"),
            object: SKETCH_ID,
            degrees_of_freedom: 0,
            redundant: &redundant,
        }]);
        let page = drawn.join("\n");
        let at = |needle: &str| drawn.iter().position(|word| word == needle);
        for (what, found) in [
            ("the first identifier", at(FIRST_REDUNDANT)),
            ("what the first constraint says", at(FIRST_SAYS)),
            ("the second identifier", at(SECOND_REDUNDANT)),
            ("what the second constraint says", at(SECOND_SAYS)),
        ] {
            assert!(found.is_some(), "{what} never reached the screen:\n{page}");
        }
        assert!(
            at(FIRST_REDUNDANT) < at(FIRST_SAYS)
                && at(FIRST_SAYS) < at(SECOND_REDUNDANT)
                && at(SECOND_REDUNDANT) < at(SECOND_SAYS),
            "an explanation does not sit under the identifier it belongs to: {drawn:?}"
        );
        // Whole, not shortened to fit. The sentence names two curves by the
        // identifiers a document stores, and half of one of those is not
        // something a person can look up.
        assert!(
            drawn.iter().any(|word| word == FIRST_SAYS),
            "the first explanation reached the screen cut short:\n{page}"
        );
        assert_eq!(
            drawn.iter().filter(|word| *word == "Says").count(),
            2,
            "two repeated constraints were explained some other number of times: {drawn:?}"
        );
    }

    #[test]
    fn two_sketches_keep_their_own_facts_and_their_own_order() {
        let first = SolvedSketch {
            name: Some("Loose"),
            object: SKETCH_ID,
            degrees_of_freedom: 2,
            redundant: &[],
        };
        let second_redundant = [RedundantExplanation {
            identifier: SECOND_REDUNDANT,
            says: SECOND_SAYS,
        }];
        let second = SolvedSketch {
            name: Some("Sized"),
            object: FIRST_REDUNDANT,
            degrees_of_freedom: 0,
            redundant: &second_redundant,
        };
        let page = page(&[first, second]);
        let loose = page.find("Loose").expect("the first sketch was drawn");
        let sized = page.find("Sized").expect("the second sketch was drawn");
        assert!(
            loose < sized,
            "the section reordered the sketches, so a row no longer names the sketch above \
             it:\n{page}"
        );
        // The second sketch's own repeated constraint sits after the second
        // sketch's name and not after the first's, which is what makes this a
        // statement about whose facts these are.
        let repeated = page
            .find(SECOND_REDUNDANT)
            .expect("the repeated constraint was drawn");
        assert!(
            repeated > sized,
            "one sketch was given the repeated constraint of another:\n{page}"
        );
        assert!(
            page.contains("Fully constrained") && page.contains("Under-constrained"),
            "two sketches in different states were described as being in one:\n{page}"
        );
    }

    #[test]
    fn nothing_in_the_section_can_be_pressed() {
        // What this section may paint: the words themselves. A row that had
        // become choosable, or a control that had appeared beside one, would
        // paint itself a background or a frame first, and that is what this
        // refuses – before a click, under one, and after it.
        //
        // Asked of the marks rather than of egui's record of which widget a
        // click reached: a plain label is already click-sensing, because that
        // is how a person selects the text in one, so that record cannot tell
        // a label from a button. This was found by the mutation campaign of
        // 21B-3b2, which turned every row of this section into a button and
        // was not noticed.
        //
        // The other half of the same statement is the signature: this panel
        // returns nothing, so there is nothing a caller could apply even if a
        // press did reach something.
        let redundant = repeats();
        let sketches = [
            settled(),
            SolvedSketch {
                name: None,
                object: FIRST_REDUNDANT,
                degrees_of_freedom: 3,
                redundant: &redundant,
            },
        ];
        let context = egui::Context::default();
        let mut warm = context.run_ui(egui::RawInput::default(), |_| {});
        warm.textures_delta.clear();

        let mut said = None;
        for step_y in 0..40 {
            let at = egui::Pos2::new(60.0, step_y as f32 * 5.0);
            for events in [
                vec![egui::Event::PointerMoved(at)],
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                }],
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
            ] {
                let mut output = context.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| sketch_solves_panel(ui, &sketches),
                );
                output.textures_delta.clear();
                let painted = marks(&output.shapes);
                // One mark that is not a word, at every position and in every
                // pass: the bar this section scrolls itself with, which is
                // deliberate and is not a way to choose anything. Anything a
                // person could press to act on a sketch would be one more.
                assert_eq!(
                    painted.get("furniture"),
                    Some(&1),
                    "a pointer at {at:?} found something in this section that paints itself \
                     like a control: {painted:?}"
                );

                let mut words = Vec::new();
                for clipped in &output.shapes {
                    collect_text(&clipped.shape, &mut words);
                }
                match &said {
                    None => said = Some(words),
                    Some(before) => assert_eq!(
                        &words, before,
                        "a pointer at {at:?} changed what this section says"
                    ),
                }
            }
        }
    }

    #[test]
    fn the_section_speaks_no_solver_and_no_debug_vocabulary() {
        let redundant = repeats();
        let page = page(&[
            settled(),
            SolvedSketch {
                name: None,
                object: FIRST_REDUNDANT,
                degrees_of_freedom: 3,
                redundant: &redundant,
            },
        ]);
        // What a solve knows and a window may never print: the numbers one
        // call to one library gave itself, and the shape of a value rather
        // than a sentence.
        for forbidden in [
            "ConstraintId",
            "PointId",
            "SketchSolveReport",
            "SketchSolveFacts",
            "StableEntityId",
            "ObjectId",
            "degrees_of_freedom",
            "redundant:",
            "Some(",
            "None(",
            "[",
            "planegcs",
            "session",
            "ordinal",
            "index",
            // The document's own type names, and the punctuation a derived
            // Debug puts around a value. A section that printed either would
            // be showing the shape of what it holds rather than saying it.
            "SketchConstraintRule",
            "SketchPointRef",
            "SketchSegmentRef",
            "RedundantConstraint",
            "{",
            "}",
        ] {
            assert!(
                !page.contains(forbidden),
                "the section printed {forbidden:?}, which means something only to one solve:\
                 \n{page}"
            );
        }
    }

    // -----------------------------------------------------------------
    // What the window says about an Open that failed
    // -----------------------------------------------------------------

    const ATTEMPTED_FILE: &str = "impossible.fcad";
    const CONFLICT_SKETCH: &str = "01930f2c-1a2b-7c3d-8e4f-444444444444";

    /// Two constraints that cannot both hold, as the application words them.
    fn disagreeing() -> [ConflictingRule<'static>; 2] {
        [
            ConflictingRule {
                identifier: FIRST_REDUNDANT,
                says: FIRST_SAYS,
            },
            ConflictingRule {
                identifier: SECOND_REDUNDANT,
                says: SECOND_SAYS,
            },
        ]
    }

    /// Every word the failure section actually drew, in the order it drew them.
    fn failure_words(failure: Option<OpenFailure<'_>>) -> Vec<String> {
        let context = egui::Context::default();
        // A frame first, so the fonts are loaded: without them every line
        // comes back empty and an assertion about absence would pass.
        let mut warm = context.run_ui(egui::RawInput::default(), |_| {});
        warm.textures_delta.clear();

        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            open_failure_panel(ui, failure);
        });
        output.textures_delta.clear();
        let mut words = Vec::new();
        for clipped in output.shapes {
            collect_text(&clipped.shape, &mut words);
        }
        words
    }

    #[test]
    fn a_failed_open_is_laid_out_as_the_attempt_it_was() {
        let rules = disagreeing();
        let words = failure_words(Some(OpenFailure {
            file: ATTEMPTED_FILE,
            sketch: CONFLICT_SKETCH,
            constraints: &rules,
        }));

        // The heading, what went wrong, which file it went wrong on, which
        // sketch inside it, and then both halves of each constraint in the
        // order they arrived. Positions rather than presence: two right lines
        // in the wrong order read as an account of something else.
        assert_eq!(
            words,
            vec![
                "Open failed".to_owned(),
                "Problem".to_owned(),
                "Constraint conflict".to_owned(),
                "File".to_owned(),
                ATTEMPTED_FILE.to_owned(),
                "Sketch".to_owned(),
                CONFLICT_SKETCH.to_owned(),
                "Constraint".to_owned(),
                FIRST_REDUNDANT.to_owned(),
                "Says".to_owned(),
                FIRST_SAYS.to_owned(),
                "Constraint".to_owned(),
                SECOND_REDUNDANT.to_owned(),
                "Says".to_owned(),
                SECOND_SAYS.to_owned(),
            ],
            "the section did not say, in order, what failed, where, and why"
        );
    }

    #[test]
    fn a_window_with_no_failed_attempt_draws_no_such_section() {
        assert!(
            failure_words(None).is_empty(),
            "a window that has not failed to open anything drew an account of a failure"
        );
    }

    #[test]
    fn nothing_in_a_failed_opens_account_can_be_pressed() {
        // What this section may paint: the words themselves, and the one line
        // it draws under itself. A row that had become a button, a control
        // that had appeared beside one, or a row that could be chosen would
        // paint itself a background or a frame first, and that is what this
        // refuses – before a click, under one, and after it.
        //
        // Asked of the marks rather than of egui's record of which widget a
        // click reached: a plain label is already click-sensing, because that
        // is how a person selects the text in one, so that record cannot tell
        // a label from a button.
        //
        // The other half of the same statement is the signature: this panel
        // returns nothing, so there is nothing a caller could apply even if a
        // press did reach something.
        let rules = disagreeing();
        let context = egui::Context::default();
        let mut warm = context.run_ui(egui::RawInput::default(), |_| {});
        warm.textures_delta.clear();
        let draw = |ui: &mut egui::Ui| {
            open_failure_panel(
                ui,
                Some(OpenFailure {
                    file: ATTEMPTED_FILE,
                    sketch: CONFLICT_SKETCH,
                    constraints: &rules,
                }),
            );
        };

        let mut said = None;
        for step in 0..40 {
            let at = egui::Pos2::new(60.0, step as f32 * 8.0);
            for events in [
                vec![egui::Event::PointerMoved(at)],
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                }],
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
            ] {
                let mut output = context.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    draw,
                );
                output.textures_delta.clear();
                let painted = marks(&output.shapes);
                assert_eq!(
                    painted.get("furniture"),
                    None,
                    "a pointer at {at:?} found something in this section that paints itself \
                     like a control: {painted:?}"
                );

                // And the words are the words, whatever the pointer does.
                let mut words = Vec::new();
                for clipped in &output.shapes {
                    collect_text(&clipped.shape, &mut words);
                }
                match &said {
                    None => said = Some(words),
                    Some(before) => assert_eq!(
                        &words, before,
                        "a pointer at {at:?} changed what this section says"
                    ),
                }
            }
        }
    }
}
