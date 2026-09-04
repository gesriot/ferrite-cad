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
mod rebuild;
mod render;
mod sample;
mod topology;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use ferritecad_document::{CacheStore, DOCUMENT_EXTENSION, Document};
use ferritecad_jobs::Existing;
use ferritecad_kernel::TessellationParams;
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
    /// Show a document's metadata, objects, graph and references.
    Inspect(DocumentArgs),
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
}

#[derive(Debug, Args)]
struct ExportFbxArgs {
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
          default_values_t = [60.0, 40.0, 10.0])]
    size: Vec<f64>,
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
            report(&error);
            ExitCode::from(EXIT_FAILED)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Create(args) => create(args),
        Command::Inspect(args) => {
            let document = Document::open(&args.path)?;
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
        Command::ExportStl(args) => export::export_stl(args),
        Command::ExportFbx(args) => export_fbx::export_fbx(args),
        Command::Rebuild(args) => rebuild::rebuild(args),
        Command::PrintTopology(args) => topology::print_topology(args),
        Command::ImportStep(args) => import::import_step(args),
    }
}

fn create(args: CreateArgs) -> Result<ExitCode> {
    if args.path.extension().and_then(|e| e.to_str()) != Some(DOCUMENT_EXTENSION) {
        eprintln!(
            "note: {} does not end in .{DOCUMENT_EXTENSION}",
            args.path.display()
        );
    }

    let length_unit: Unit = args.length_unit.parse()?;
    let angle_unit: Unit = args.angle_unit.parse()?;
    let mut document = Document::create_with(&args.path, length_unit, angle_unit)?;

    if args.sample {
        let [width, depth, height] = args.size.as_slice() else {
            return Err(CadError::input("--size takes exactly three numbers"));
        };
        sample::populate(&mut document, *width, *depth, *height)?;
    }

    let path = document.path().to_path_buf();
    let id = document.meta().document_id;
    document.close()?;

    println!("created {} ({id})", path.display());
    Ok(ExitCode::SUCCESS)
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
fn report(error: &CadError) {
    eprintln!("error [{}]: {error}", error.kind());

    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }
}
