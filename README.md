# FerriteCAD

A local, parametric mechanical CAD for Windows, Linux and macOS. No cloud, no
account, no proprietary container you cannot read back.

**Status: early. There is no user interface and no geometry yet.** What exists
is the document format and the tooling around it. See
[`docs/implementation-plan.md`](docs/implementation-plan.md) for what comes when,
and please read the honest scope note at the end of this file before forming
expectations.

## Building

```sh
cargo build
cargo test
```

Nothing so far needs Open CASCADE, so this builds with a Rust toolchain alone.
The kernel arrives in stage 2; its build recipe is
[`docs/build-occt.md`](docs/build-occt.md).

## Trying it

```sh
cargo run -p ferritecad-cli -- create part.fcad --sample
cargo run -p ferritecad-cli -- inspect part.fcad
cargo run -p ferritecad-cli -- validate part.fcad
cargo run -p ferritecad-cli -- dump-graph part.fcad --format dot | dot -Tsvg > graph.svg
```

`create --sample` builds a plate: a datum plane, a rectangular profile, an
extrusion and the topology references naming that extrusion's faces. Nothing is
evaluated into geometry yet — the point is that the *model* round-trips.

## How a document is put together

A document is one SQLite file, `part.fcad`. It holds only what cannot be
recomputed:

| Table | Holds |
| --- | --- |
| `meta` | format version, document UUID, display units, timestamps |
| `capabilities` | what a reader must implement to write this document |
| `objects` | sketches, features, bodies, parameters, as CBOR envelopes |
| `deps` | the dependency graph edges, with the role of each |
| `topology_refs` | semantic names for produced geometry |

Everything derived — B-Rep, tessellation, entity mappings, previews — lives in
a separate `part.fcad-cache` sidecar. **Deleting the sidecar can cost you time
and nothing else.** That is a tested invariant, not an aspiration.

Three properties the format commits to:

- **Nothing is referenced by index.** A face is named by what produced it and
  why — "the cap of this extrusion", "every face raised from this profile
  segment" — never by position in a traversal. An index-based reference is the
  thing that makes a parametric model silently point at the wrong face after an
  upstream edit.
- **What is not understood is preserved, not dropped.** An object whose type
  this build has never heard of is carried through save and reload byte for
  byte, and a document requiring a capability this build lacks opens read-only
  rather than being rewritten lossily.
- **A refusal beats a wrong answer.** An unresolvable reference stops the
  rebuild and says so. It never picks a neighbouring face that happens to look
  plausible.

## Layout

```
crates/
  ferritecad-types/     identifiers, units, tolerances, errors, canonical hashing
  ferritecad-document/  SQLite container, CBOR envelopes, graph, cache sidecar
  ferritecad-cli/       create, inspect, validate, dump-graph, clear-cache
docs/
```

Dependencies only point downwards. The document layer knows nothing about
geometry kernels or user interfaces, and it never will.

## Scope, honestly

FerriteCAD is not a SolidWorks or KOMPAS-3D replacement and calling it one
would be dishonest. The target for the first beta is narrower and worth
stating plainly: **a fast local parametric part modeller** — sketch, extrude,
revolve, cut, fillet, chamfer, shell, pattern; save, reopen and rebuild
without cache; STEP in and out; STL out.

Assemblies, drawings, ESKD, sheet metal, scripting and plugins are all out of
scope until that works.

## Licence

MIT, see [`LICENSE`](LICENSE). Open CASCADE is LGPL-2.1 with the Open CASCADE
exception and, by project policy, will be linked dynamically only; see
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).
