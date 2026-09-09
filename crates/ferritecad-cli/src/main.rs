// SPDX-License-Identifier: MIT
//! `ferritecad` — inspection and validation tools for native documents.
//!
//! These commands exist before the user interface does, so the document format
//! can be exercised, diffed and regression-tested on its own. Anything the
//! interface will later need to know about a document should be answerable
//! here first.

mod export;
mod export_fbx;
mod import;
mod json;
mod rebuild;
mod render;
mod topology;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use ferritecad_document::{CacheStore, DOCUMENT_EXTENSION, Document};
use ferritecad_jobs::{CreateDocumentRequest, Existing, NewDocument, PlateSize, create_document};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_types::{CadError, Result, Unit};

/// Exit code for a document that failed validation, as opposed to a command
/// that could not run at all.
const EXIT_INVALID: u8 = 1;
const EXIT_FAILED: u8 = 2;
/// A document that opened and rebuilt, and whose stored names no longer all
/// find geometry. Distinct from both of the above: nothing went wrong with the
/// command, and the document is not malformed — it has simply lost a name.
const EXIT_UNRESOLVED: u8 = 3;
/// An import that produced a complete document, with things noticed on the way.
///
/// Separate from success because a script should be able to tell the two apart
/// without parsing prose, and separate from failure because the document is
/// there and is whole. It does not mean the file is worse than one that exits
/// zero — only that this reader said something about it.
const EXIT_NOTICED: u8 = 4;
/// A file the importer would not read at all. Nothing was written.
const EXIT_REJECTED: u8 = 5;
/// An export that produced a whole file, and could not give every definition
/// of the document triangles.
///
/// Separate from success because the file is not the whole model and a script
/// must be able to tell without parsing prose, and separate from failure
/// because the file is there, was published, and every definition kept its
/// place in it. Distinct from every code above: none of them means "there is
/// an export, and here is what is missing from it".
const EXIT_PARTIAL: u8 = 6;

/// What this command tells a person to do about a file that is already there.
///
/// One sentence, in one place, for all three commands that publish a file. It
/// travels with the publication rather than living inside it, because the
/// window that publishes on exactly the same terms has no `--force` to offer
/// and must not print one.
const REPLACE_ADVICE: &str = "pass --force to replace it";

/// What this command tells a person about a destination `create` will not take.
///
/// Deliberately not [`REPLACE_ADVICE`]: `create` has no `--force`, so naming
/// one would send somebody looking for a flag that does not exist. The sentence
/// is the one this command has always printed, now finishing the refusal the
/// shared publication makes rather than a check inside the document layer.
const CREATE_TAKEN_ADVICE: &str = "creating would destroy it";

/// What `--force` means at the moment a file is published.
///
/// Every command here publishes on the same terms: without the flag the
/// publication is an atomic no-clobber that refuses in this command's own
/// words, and with it the replacement is one the user asked for.
fn replacing(force: bool) -> Existing<'static> {
    if force {
        Existing::Replace
    } else {
        Existing::Keep {
            advice: REPLACE_ADVICE,
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "ferritecad", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a new document.
    Create(CreateArgs),
    /// Change one constant Blind extrusion and save a new .fcad of the same model.
    /// Preserves identities; never overwrites source or output. Requires a kernel.
    EditExtrude(EditExtrudeArgs),
    /// Show a document's metadata, objects, graph and references.
    Inspect(InspectArgs),
    /// Check that a document is internally consistent and rebuildable.
    Validate(DocumentArgs),
    /// Print the dependency graph.
    DumpGraph(DumpGraphArgs),
    /// Delete a document's regenerable cache sidecar.
    ClearCache(DocumentArgs),
    /// Rebuild a document and write one of its solids as binary STL.
    ExportStl(ExportStlArgs),
    /// Rebuild a document and write the whole model as FBX 7.4 ASCII.
    ///
    /// Every definition and every placement, with the assembly hierarchy and
    /// the local transforms the sources recorded, in one file another program
    /// opens as a model rather than as a lump of triangles. An imported part is
    /// read from the bytes the document stores, so the file it came from need
    /// not exist any more.
    ///
    /// A definition this build cannot turn into triangles keeps its place in
    /// the hierarchy, says so in the file, and is reported on standard error.
    /// Such an export is published and exits 6 rather than 0: the file is real,
    /// and it is not the whole model.
    ExportFbx(ExportFbxArgs),
    /// Rebuild a document from scratch and report what it produced.
    Rebuild(RebuildArgs),
    /// Rebuild a document and report what each stored reference resolves to.
    PrintTopology(DocumentArgs),
    /// Read a STEP file into a new document, source bytes and all.
    ImportStep(ImportStepArgs),
}

#[derive(Debug, Args)]
struct EditExtrudeArgs {
    /// Existing .fcad, read without migration or modification.
    source: PathBuf,
    /// Exact feature UUID printed by inspect (never a name or index).
    #[arg(long)]
    feature: ferritecad_types::ObjectId,
    /// New finite, positive distance in millimetres, regardless of display units.
    #[arg(long, allow_hyphen_values = true)]
    distance_mm: f64,
    /// New .fcad path. Existing files are refused; there is no --force.
    #[arg(short, long)]
    output: PathBuf,
    /// Optional complete content version printed by inspect; refuses stale input.
    #[arg(long)]
    expect_version: Option<ferritecad_types::ContentHash>,
    /// Emit one JSON v1 result or execution error. Argument errors remain clap text.
    /// Source and output paths must be UTF-8 in JSON mode.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct InspectArgs {
    /// Path to the document. Read without migration, writes or a kernel.
    path: PathBuf,
    /// Emit the JSON v1 extrusion-edit catalog, rather than the full text report.
    /// Argument errors and help remain clap text. Paths must be UTF-8 in JSON mode.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ImportStepArgs {
    /// Path to the STEP file. Opened once, read, and never written to.
    path: PathBuf,

    /// Path of the document to create.
    #[arg(long, short)]
    output: PathBuf,

    /// What to call the imported object in the document.
    ///
    /// Defaults to the STEP file's own name. The names the file gave its parts
    /// are kept as the file gave them and are not affected by this.
    #[arg(long)]
    name: Option<String>,

    /// Replace the output document if it already exists.
    #[arg(long)]
    force: bool,

    /// Emit JSON v1 publication facts or a typed reader rejection.
    /// Argument errors and help remain clap text. Paths must be UTF-8.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RebuildArgs {
    /// Path to the document.
    path: PathBuf,

    /// Rebuild every feature, consulting no cache.
    ///
    /// Required. The cached path exists but is not offered here yet, and
    /// making it the default would hide which one produced the answer.
    #[arg(long)]
    cold: bool,
}

#[derive(Debug, Args)]
struct ExportStlArgs {
    /// Path to the document.
    path: PathBuf,

    /// Path to write the mesh to.
    #[arg(long, short)]
    output: PathBuf,

    /// Which body to export, by name or identifier.
    ///
    /// Optional only while a document holds exactly one body. With several,
    /// this is required rather than guessed.
    #[arg(long)]
    solid: Option<String>,

    /// Millimetres of chord error allowed between the mesh and the surface.
    #[arg(long, default_value_t = TessellationParams::DEFAULT_LINEAR)]
    linear_deflection: f64,

    /// Radians of angular error allowed between neighbouring facets.
    #[arg(long, default_value_t = TessellationParams::DEFAULT_ANGULAR)]
    angular_deflection: f64,

    /// Replace the output file if it already exists.
    #[arg(long)]
    force: bool,

    /// Emit the versioned JSON v1 result instead of the text report.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ExportFbxArgs {
    /// Emit the versioned JSON publication report (partial publication exits 6).
    #[arg(long)]
    json: bool,

    /// Path to the document. Opened read-only and never written to.
    path: PathBuf,

    /// Path to write the FBX to.
    #[arg(long, short)]
    output: PathBuf,

    /// Replace the output file if it already exists.
    ///
    /// Never enough to make the document itself an acceptable output.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct DocumentArgs {
    /// Path to the document.
    path: PathBuf,
}

#[derive(Debug, Args)]
struct CreateArgs {
    /// Path to write. Refuses to overwrite an existing file.
    path: PathBuf,

    /// Unit to display lengths in. Values are always stored in millimetres.
    #[arg(long, default_value = "mm")]
    length_unit: String,

    /// Unit to display angles in. Values are always stored in radians.
    #[arg(long, default_value = "deg")]
    angle_unit: String,

    /// Populate the document with a sample plate: a plane, a rectangular
    /// profile, an extrusion and its topology references.
    #[arg(long)]
    sample: bool,

    /// Sample plate size in millimetres, as width, depth and height.
    #[arg(long, num_args = 3, value_names = ["WIDTH", "DEPTH", "HEIGHT"],
          default_values_t = [PlateSize::DEFAULT.width, PlateSize::DEFAULT.depth, PlateSize::DEFAULT.height])]
    size: Vec<f64>,

    /// Emit one JSON v1 result or execution error. Argument errors remain clap text.
    /// The output path must be UTF-8 in JSON mode.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct DumpGraphArgs {
    path: PathBuf,

    #[arg(long, value_enum, default_value_t = GraphFormat::Text)]
    format: GraphFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum GraphFormat {
    /// Indented evaluation order.
    Text,
    /// Graphviz DOT, for `dot -Tsvg`.
    Dot,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            let _ = report(&error);
            ExitCode::from(EXIT_FAILED)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Create(args) if args.json => Ok(json::emit(
            json::Operation::Create,
            create_result(args).map(json::Created::from),
        )),
        Command::Create(args) => create(args),
        Command::EditExtrude(args) if args.json => Ok(json::emit(
            json::Operation::EditExtrude,
            edit_extrude_result(args).map(json::Edited::from),
        )),
        Command::EditExtrude(args) => edit_extrude(args),
        Command::Inspect(args) if args.json => Ok(json::emit(
            json::Operation::Inspect,
            json::inspect(&args.path),
        )),
        Command::Inspect(args) => {
            let document = Document::open_read_only(&args.path)?;
            render::inspect(&document)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Validate(args) => {
            let document = Document::open(&args.path)?;
            let report = document.validate()?;
            render::validation(&args.path, &report);
            Ok(if report.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(EXIT_INVALID)
            })
        }
        Command::DumpGraph(args) => {
            let document = Document::open(&args.path)?;
            match args.format {
                GraphFormat::Text => render::graph_text(&document)?,
                GraphFormat::Dot => render::graph_dot(&document)?,
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::ClearCache(args) => clear_cache(args),
        Command::ExportStl(args) if args.json => Ok(json::emit(
            json::Operation::ExportStl,
            export::export_stl_result(args).map(json::ExportedStl::from),
        )),
        Command::ExportStl(args) => export::export_stl(args),
        Command::ExportFbx(args) if args.json => Ok(json::emit_with_exit(
            json::Operation::ExportFbx,
            export_fbx::export_fbx_result(&args).map(|exported| {
                let exit = export_fbx::exit_code(exported.report());
                (json::ExportedFbx::from(&exported), exit)
            }),
        )),
        Command::ExportFbx(args) => export_fbx::export_fbx(args),
        Command::Rebuild(args) => rebuild::rebuild(args),
        Command::PrintTopology(args) => topology::print_topology(args),
        Command::ImportStep(args) if args.json => {
            Ok(json::emit_import(import::import_step_result(&args)))
        }
        Command::ImportStep(args) => import::import_step(args),
    }
}

fn create(args: CreateArgs) -> Result<ExitCode> {
    let created = create_result(args)?;
    println!(
        "created {} ({})",
        created.destination().display(),
        created.document_id()
    );
    Ok(ExitCode::SUCCESS)
}

/// Both output modes submit exactly the same request and publish exactly once.
fn create_result(args: CreateArgs) -> Result<ferritecad_jobs::CreatedDocument> {
    use std::io::Write;

    if args.json {
        json::require_utf8_path(&args.path)?;
    }
    if args.path.extension().and_then(|e| e.to_str()) != Some(DOCUMENT_EXTENSION) {
        let note = format!(
            "note: {} does not end in .{DOCUMENT_EXTENSION}",
            args.path.display()
        );
        if args.json {
            // A closed diagnostic pipe must not panic or block the JSON report.
            let _ = writeln!(std::io::stderr().lock(), "{note}");
        } else {
            eprintln!("{note}");
        }
    }

    let length_unit: Unit = args.length_unit.parse()?;
    let angle_unit: Unit = args.angle_unit.parse()?;

    let content = if args.sample {
        let [width, depth, height] = args.size.as_slice() else {
            return Err(CadError::input("--size takes exactly three numbers"));
        };
        NewDocument::SamplePlate(PlateSize {
            width: *width,
            depth: *depth,
            height: *height,
        })
    } else {
        NewDocument::Empty
    };

    // The default context: this command offers no way to change its mind part
    // way through, so there is nothing to withdraw the request with. The
    // operation takes one anyway because the window does have such a way, and
    // a second entry point for the interface that does not would be a second
    // creation.
    create_document(
        CreateDocumentRequest::new(&args.path, content, CREATE_TAKEN_ADVICE)
            .displaying(length_unit, angle_unit),
        &OperationContext::default(),
    )
}

fn clear_cache(args: DocumentArgs) -> Result<ExitCode> {
    let document = Document::open(&args.path)?;
    let cache = document.cache_path();
    document.close()?;

    let existed = cache.exists();
    CacheStore::discard(&cache)?;

    if existed {
        println!("removed {}", cache.display());
    } else {
        println!("no cache sidecar at {}", cache.display());
    }
    Ok(ExitCode::SUCCESS)
}

/// Prints an error with its full cause chain.
///
/// The chain is what makes a storage failure diagnosable: "opening document"
/// on its own says nothing, "opening document: unable to open database file"
/// says everything.
fn report(error: &CadError) -> std::io::Result<()> {
    use std::io::Write;

    let mut stderr = std::io::stderr().lock();
    writeln!(stderr, "error [{}]: {error}", error.kind())?;

    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        writeln!(stderr, "  caused by: {cause}")?;
        source = cause.source();
    }
    Ok(())
}

fn edit_extrude(args: EditExtrudeArgs) -> Result<ExitCode> {
    let edited = edit_extrude_result(args)?;
    println!(
        "saved {} ({}, feature {})",
        edited.destination.display(),
        edited.document_id,
        edited.feature
    );
    Ok(ExitCode::SUCCESS)
}

/// Both output modes submit exactly the same request and publish exactly once.
fn edit_extrude_result(args: EditExtrudeArgs) -> Result<ferritecad_jobs::EditedDocument> {
    if args.json {
        // Before opening a source or publishing: an unrepresentable output
        // must not turn a successful edit into an apparent operation refusal.
        json::require_utf8_path(&args.source)?;
        json::require_utf8_path(&args.output)?;
    }
    let source = ferritecad_jobs::read_extrude_source(&args.source)?;
    let mut expected = source.version;
    if let Some(version) = args.expect_version {
        expected.content = version;
    }
    let request = ferritecad_jobs::EditExtrudeRequest {
        source: args.source,
        expected,
        feature: args.feature,
        distance_mm: args.distance_mm,
        destination: args.output,
    };
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    ferritecad_jobs::edit_extrude_copy(&request, &mut kernel, &OperationContext::default())
}
