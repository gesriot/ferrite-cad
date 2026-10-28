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

The isolated naming experiment measures a cold rebuild and then generative
naming separately, including every intermediate feature result:

```sh
cargo run -p ferritecad-bench --features planegcs --release --example naming -- \
  /tmp/ferritecad-naming-corpus /tmp/ferritecad-naming.json 5
```

Use the native OCCT/PlaneGCS environment described above. The first run writes
the corpus, including Chamfer; subsequent runs reuse the same document bytes.
The JSON contains raw rebuild/naming/total samples, counts for every feature,
all ambiguous primary names and comparisons with stored references and consumed
swept edges. Run timing samples sequentially without other builds or tests.

## Long single-body histories

`--long` selects a separate corpus; it does not extend the ordinary corpus or
its existing smoke test. There are two families at N = 100, 300 and 1000 Cuts:

- `pockets_N`: independent radius-1 mm blind pockets on a 4 mm grid, 32 columns.
- `rework_N`: concentric blind pockets with radii `1 + 0.01 * k` mm,
  `k = 0..N-1`. Each larger pocket absorbs its predecessor's wall and floor.

Both start with the same 140 by 140 by 10 mm plate and use a 4 mm blind depth.
The grid, plate dimensions and depth are fixed across N. Each document contains
N + 1 feature results: position zero is the plate, positions 1 through N are
Cuts. These unconstrained documents also work without PlaneGCS.

Build once, finish all other builds/tests, then measure on a developer Mac:

```sh
cargo build -p ferritecad-bench --release --all-features
target/release/ferritecad-bench --long --runs 3 \
  --corpus-dir /tmp/long-histories-run --out /tmp/long-histories.json
```

Use the native library environment from `CONTRIBUTING.md`. The driver starts
each case sequentially in a separate process under the existing
`tools/watch-viewer-memory.py`, with a 6144 MiB physical-footprint limit and
1800-second case deadline. `/usr/bin/time -l` supplies the process's maximum
RSS in bytes. The watchdog separately records sampled process-tree RSS and
physical footprint; these are different metrics and are not added together.
The guarded driver requires macOS because that watchdog uses macOS APIs.
Generation and the native short-history tests remain portable to all CI hosts.

`--long-case pockets_300` selects one case, still in an isolated guarded
process. Case directories must be new so earlier evidence cannot be overwritten.
`--long --generate-only --corpus-dir DIR` writes the six documents without
measuring them. The existing documents and their generator stay unchanged.

The output JSON retains raw samples, counters, median summaries and growth
exponents. Each case directory retains its document, cache, watchdog JSONL,
stdout/stderr and incremental result. All are local outputs; do not commit them.
The driver prints completion status for each case. A footprint or deadline limit
is an incomplete measurement, not a zero or an extrapolated time. It records the
phase and current feature from stderr. An N = 100 deadline or a modelling failure
stops the run for investigation. Incomplete pairs have no growth exponent.

Timing scopes:

- `cold_ms`: the actual evaluator's `rebuild_cold`, excluding signature checks,
  release and fixture generation. It includes profile conversion and naming.
- `kernel_ms`: Extrude and Boolean calls within that same cold rebuild,
  including native provenance capture. Naming's topology/history queries are
  outside this interval.
- `naming_ms`: a separate replay of the public `ModelNaming` calls on the same
  stored document in a fresh kernel session. It includes model preparation,
  profile-root selection, tool/result derivation and insertion/collision checks,
  including topology/history queries. Geometry construction, archival and
  fingerprint comparisons are excluded. This is a separately sampled component,
  not a subtraction that must add exactly to the evaluator's cold time.
- `naming_k10_ms`, `naming_k50_ms`, `naming_k100_ms`: the mean naming time of ten
  trailing Cuts ending at N/10, N/2 and N, then the median across runs. Model
  preparation is excluded from these windows.
- `cache_ms`: the actual evaluator's `rebuild_cached` with a primed cache, in a
  fresh kernel session. Every feature must report `Hit`. Cold, replay and cached
  results must agree on all derivation keys, primary name multiplicities and
  complete ancestry expressed through primary names. No fallback rebuild counts
  as a cache sample.

There is no untimed warm-up of an entire long history. Each sample runs cold,
naming replay, then cached rebuild, releasing each model before the next one.
The first replay writes the exact production archive format with production keys
outside the naming timer. Later replays do not write it. Kernel sessions retain
all intermediate results and tools as the evaluator does. The per-case RSS peak
covers generation, measurements, fingerprint checks and cache creation, rather
than claiming to isolate the naming maps' allocation alone.

Counters sum public `FeatureNaming` fields over all N + 1 results. `derivations`
counts map entries, `ancestry_members` sums set sizes, and `entities` counts
physical bindings. `ancestry_rows` and `empty_ancestry_rows` also expose retained
deleted meanings. `unique_derivations` is the union of digest keys;
`distinct_derivation_allocations` counts distinct live `Arc` pointees, so shared
node payloads are not confused with the copied maps. `tip_entities` counts only
the final result. These counters describe the cold model.

`cache_entry_bytes` sums serialized archive payload lengths after verifying
their stored bytes; `last_entry_bytes` is the final feature's payload length.
`cache_file_bytes` is the closed SQLite file size, including storage overhead.
The driver calculates `ln(m2/m1) / ln(N2/N1)` for every positive measured metric
between adjacent N in each family. One indicates linear growth, two quadratic;
missing or zero values have no exponent. A killed run's sampled RSS is a lower
bound and is never substituted for `/usr/bin/time`'s completed maximum RSS.

The ordinary test suite additionally builds N = 10 of each family and checks
all cold/replayed/cache names without timing thresholds. It skips only when OCCT
is absent and not required; no solver or GPU is needed for these short cases.
