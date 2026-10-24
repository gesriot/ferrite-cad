# Modelling benchmarks

This unpublished workspace tool generates its documents through the document
writer and shared fixtures. It is outside the two product dependency graphs,
packages, notices and inventories. It adds no third-party packages to the lockfile.

With the native library environment from `CONTRIBUTING.md`:

```sh
cargo run -p ferritecad-bench --release --all-features -- --out results.json
```

The default is five runs per operation. `--runs N` changes that count. JSON and
the stdout table contain median, minimum, maximum and count in milliseconds.
Timings never gate CI; the ordinary native smoke test runs one small case once.

Each run generates a plate, a plate with 16 cuts, a constrained plate with four
fillets, 10 and 50 independent bodies with four cuts each, and a partial Revolve.
The bodies have separate object and curve identities. Their profiles are spaced
on XY so the existing cut policy still applies.

Cold and warm rebuilds release their shapes inside the timed interval. The warm
cache is populated before timing. Open includes read, rebuild and tessellation.
Apply starts with a session whose picture has already been loaded; timing ends
after the returned picture is available and the version is committed. Height,
fillet radius and Revolve angle use the common edit transaction. A plate vertex
scenario moves the first vertex 1 mm in X and updates both incident Lines in
one transaction. Models with several bodies select an extrusion by UUID. Undo
starts with one accepted edit and includes a fresh scene load and commit. STL
selects one body and includes the full document rebuild, tessellation and atomic
publication. There is no GPU upload in these timings.

For interactive checks, retain freshly generated documents:

```sh
cargo run -p ferritecad-bench --release --all-features -- \
  --generate-only --corpus-dir /tmp/ferritecad-corpus
```

This also writes a two-fillet plate and a broken constrained two-fillet plate for
refusal and repair checks. Run the native viewer only through
`tools/watch-viewer-memory.py`, as required by `CONTRIBUTING.md`.

A session picture establishes the baseline's resolved references and unbuilt
objects. Each modelling step cold-rebuilds once, checks references and all new
failures, and tessellates that same result before publishing. CLI copy jobs have
no session facts and still rebuild twice. Open, tab switching, model Undo/Redo
and export each rebuild once. Checkpoint changes that keep the picture retain
the existing path. Neither the history nor these benchmarks cache scenes.
