# FerriteCAD

A local, parametric mechanical CAD for Windows, Linux and macOS. No cloud, no
account, no proprietary container you cannot read back.

**Status: early.** There is a document format with its tooling, geometry
through Open CASCADE, STEP import, and a viewer window that opens a `.fcad`
file, draws what it describes and lets a definition be selected and inspected.
There is no modelling in the interface: nothing can be created or edited there
yet, and everything is built through the command line. See
[`docs/implementation-plan.md`](docs/implementation-plan.md) for what comes when,
and please read the honest scope note at the end of this file before forming
expectations.

## Building

```sh
cargo build
cargo test
```

A Rust toolchain alone is enough to build and test the workspace: without Open
CASCADE the kernel adapter compiles to a stub that refuses, and the tests that
need real geometry skip themselves and say so. Building geometry for real needs
the pinned kernel, whose build recipe is
[`docs/build-occt.md`](docs/build-occt.md).

## Trying it

```sh
cargo run -p ferritecad-cli -- create part.fcad --sample
cargo run -p ferritecad-cli -- inspect part.fcad
cargo run -p ferritecad-cli -- validate part.fcad
cargo run -p ferritecad-cli -- dump-graph part.fcad --format dot | dot -Tsvg > graph.svg
```

`create --sample` builds a plate: a datum plane, a rectangular profile, an
extrusion and the topology references naming that extrusion's faces. With Open
CASCADE present, `rebuild --cold` evaluates it into geometry and `export-stl`
writes it out; without one, the document still round-trips on its own.

## Looking at a document

```sh
cargo run -p ferritecad-app --bin ferritecad-viewer -- part.fcad
```

The viewer opens the document read-only, rebuilds it, and draws it: orbit, pan
and zoom, the standard views, and a click selects what is under it and describes
it in portable terms. Clicking a face of a native body the document has a
durable name for selects that face, and the inspector says what the document
calls it: which feature made it, what it is – the end cap of that extrusion, the
side raised from that sketch segment – and how the reference selects it.
Clicking anything else selects its definition, which is the honest answer:
imported faces have no durable names, so a face of one is a face of nothing this
document could store. Selecting a part selects every placement of it, and
selecting a face marks that face in every placement, because a click names what
a thing is and never the place it happened to land on. A
list beside the model offers the same definitions by name and identity, which
is how one that is hidden, tiny or out of shot can still be chosen, and
`Frame selected` (or `F`) brings what is selected into view without changing
the direction you were looking from – the chosen face in every placement of its
part, or the whole part when a part is chosen – and `Frame all` (or `A`) does
the same for the whole model, which is the way back when panning has left it
off screen. A reference grid on the world's XY plane gives the model a floor
to sit on, with heavier lines every ten squares and coloured axes through the
origin; its spacing steps through 1, 2 and 5 millimetres and their decades as
you zoom. It is a viewing aid only: it is not in the document, not in the
model's extent, and clicking it is clicking the background.
`Hide selected` (or `H`) stops drawing what is chosen, which is how you reach a
part standing behind another one: a hidden definition leaves no pixels, cannot
be clicked or pointed at, and is not part of what `Frame all` shows. It is
hidden in every place it appears, because what is hidden is the definition and
not the spot it was drawn in, and choosing a face hides the part that face is
on. `Isolate selected` (or `I`) does the opposite: it keeps what is chosen and
stops drawing everything else, which is the way to look at one part without
picking its neighbours off one at a time. What is chosen stays chosen, down to
the face, and what was already hidden stays hidden: isolating removes
distractions and never reveals anything. `Show all` (or `U`) brings everything
back without changing what is chosen, and is the way back from either action.
Each row whose definition actually draws geometry carries one control for
whether it is drawn: `Hide` while it is on screen, `Show` once it is not, and
never both. Either acts on that row alone, in every place its definition appears,
leaving the rest of the view as you arranged it. Your selection survives
untouched, so a neighbour can be taken out of the way without giving up what
you were looking at; hiding the very thing that is chosen unchooses it, because
geometry nobody can see cannot stay chosen. Any of these drops a click or
pointer question recorded against the old picture, so geometry that has just
arrived or just left cannot answer an interaction from a frame it was not in.
`Undo visibility` takes back the last of these changes, whichever it was, and
puts the exact arrangement that preceded it back on screen. One level and no
redo: it is a way out of one accidental press, not a history of the session,
and it is deliberately not bound to a general undo key because it takes back
nothing but what is drawn. The list keeps a row for what is hidden, marked as
hidden. That is where you look when you wonder where something went. None of
this touches the document, and none of it survives opening one: a file always
opens with all of it on screen.

The button beside the views says which projection the model is drawn through
and swaps it when pressed, or `O`. `Perspective` is what an eye sees and is
where a document opens: things further away are drawn smaller, which is how a
shape is understood while it is being built. `Orthographic` is what a drawing
shows: equal things are drawn equally wherever they are and parallel edges stay
parallel, so a plan or an elevation can be measured off the screen. Swapping
keeps what you are looking at, from where, and how big it is; zooming in an
orthographic view changes the scale rather than moving the eye, and swapping
back respects that zoom.

Moving the pointer
over the model marks the face under it, and the same face wherever that
definition appears; moving it over a row of the list marks that whole
definition everywhere. Neither chooses anything, so a part or a surface can be
found before anything is selected, and what is already selected keeps its
appearance while you look around it. A click chooses a named native face as
that face and otherwise chooses its definition. Opening another document is
the only editing operation there is.

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

Everything derived – B-Rep, tessellation, entity mappings, previews – lives in
a separate `part.fcad-cache` sidecar. **Deleting the sidecar can cost you time
and nothing else.** That is a tested invariant, not an aspiration.

Three properties the format commits to:

- **Nothing is referenced by index.** A face is named by what produced it and
  why – "the cap of this extrusion", "every face raised from this profile
  segment" – never by position in a traversal. An index-based reference is the
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
  ferritecad-kernel/    the geometry contract, and a mock that satisfies it
  ferritecad-occt/      Open CASCADE behind that contract, over a C ABI shim
  ferritecad-exchange/  STEP scenes: definitions, placements, diagnostics
  ferritecad-eval/      cold and cached rebuilds of a whole document
  ferritecad-scene/     a document read into a picture, and what its parts are
  ferritecad-viewport/  camera and immutable render snapshots, no GPU
  ferritecad-viewport-gpu/  wgpu renderer, offscreen and windowed
  ferritecad-ui/        panels and the input reducer, no window
  ferritecad-app/       the window, the event loop and the wiring
docs/
```

Dependencies only point downwards. The document layer knows nothing about
geometry kernels or user interfaces, and it never will.

## Scope, honestly

FerriteCAD is not a SolidWorks or KOMPAS-3D replacement and calling it one
would be dishonest. The target for the first beta is narrower and worth
stating plainly: **a fast local parametric part modeller** – sketch, extrude,
revolve, cut, fillet, chamfer, shell, pattern; save, reopen and rebuild
without cache; STEP in and out; STL out.

Assemblies, drawings, ESKD, sheet metal, scripting and plugins are all out of
scope until that works.

## Licence

MIT, see [`LICENSE`](LICENSE). Open CASCADE is LGPL-2.1 with the Open CASCADE
exception and, by project policy, will be linked dynamically only; see
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).
