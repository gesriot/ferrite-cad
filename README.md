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
Clicking geometry for which the document has neither an exact corner name, nor
an exact edge name, nor an exact face name selects its definition, which is the
honest answer: imported corners, edges and faces have no durable names, so none
of them has a subshape identity this document could store. Selecting a part
selects every placement of it, and selecting a named face, edge or corner marks
that subshape in every placement, because a click names what a thing is and
never the place it happened to land on.
Moving the pointer over the model asks what is under it, and the answer is as
particular as the picture can be. The most particular thing is a corner: where
the faces and edges of a native body meet at one point, that one topological
vertex is marked, in every placement of the part. A corner is a point and
covers no pixel of its own, so it answers within a small square around where it
is drawn; that square is a hit area, and the dot you see is deliberately a
little smaller than it. Pointing at a corner is a question and nothing more.
The question lasts only as long as the picture on screen and is forgotten when
a new one arrives. Where two corners are drawn close enough that their squares
overlap, one answer is kept for that pixel, decided the same way every time by
drawing order; nothing here resolves between several candidates.
Clicking a marked corner selects that corner, but only when the document has an
exact durable name for it. On a native body that means the point where two
adjacent sketch segments reach one end of an extrusion; the inspector then says
what the document calls it, in the same portable terms it uses for a face and
an edge, and shows every stored name rather than picking one. It names the cap
and both segments of the joint – "Start cap vertex at the joint of profile
segments A and B" – because either half alone would name four corners of a
plate instead of one. A chosen corner is marked in every placement of its part
and is drawn differently from a mere question about the same point, so a
decision cannot be mistaken for one; a question about the edge that merely ends
there is still answered along the whole of that edge. A corner nobody named is
not a lesser corner, it is not a choice: clicking one chooses the most
particular thing the document can name instead – the edge if it is named,
otherwise the face, otherwise the part – and a corner of an imported definition
always selects the definition, because an imported corner is a corner of
nothing this document could store.
`Frame selected` brings the chosen corner itself into view rather than the edge,
the face or the part it belongs to, and `Hide selected` and `Isolate selected`
act on the part that owns it.
Off a corner, over a line where two faces of a native body
meet, that one topological edge is marked, in every placement of the part and
along the whole of it, including the part of it each of the two faces drew for
itself. Off the line, the face under the pointer is marked instead, and where
the picture cannot say which face, the part. What is marked is the edge and
nothing around it: the faces it separates and the part they belong to keep
exactly the colour they had, so it reads as a line rather than as a change of
material. A choice already made stays stronger than a question: an edge of a
selected part, or one bounding a selected face, is left alone rather than
painted over, while an edge elsewhere on the same part is still marked.
Pointing is only ever a question: it never selects anything.
Clicking a marked edge selects that edge, but only when the document has an
exact durable name for it. On a native body that means the edge where one end
of an extrusion meets a face raised from a sketch segment; the inspector then
says what the document calls it, in the same portable terms it uses for a
face, and shows every stored name rather than picking one. A chosen edge is
marked along the whole of itself, in every placement of its part and on both
of the faces that meet at it, and it is drawn differently from a mere
question about the same line so a decision cannot be mistaken for one. An
edge nobody named is not a lesser edge, it is not a choice: clicking one
selects the named face beneath it if there is one and otherwise the part, and
an edge of an imported definition always selects the definition, because an
imported edge is an edge of nothing this document could store.
`Frame selected` brings the chosen edge itself into view rather than the face
or the part it belongs to, and `Hide selected` and `Isolate selected` act on
the part that owns it, because what can be hidden is a definition. A
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
keeps what you are looking at, the viewing direction, which way is up and how
big it is. Zooming in an orthographic view changes the scale rather than moving
the eye; swapping back respects that zoom, so it may move the eye to the
distance that gives the new scale in perspective.

The wheel zooms towards the pointer rather than towards the middle of the
window. Put the pointer on the feature you want a closer look at and wind: that
feature stays where it is while everything else spreads out around it, so
getting from a whole model to one corner of it takes no compensating drag. What
is held exactly is the place where the pointer meets the plane through what you
are looking at; a surface much nearer or further than that plane drifts a
little, which is the honest cost of not reading the depth of every pixel back
from the graphics card. When the window has no current pointer position because
the pointer left or a gesture was cancelled, the wheel falls back to the middle
of the view. A wheel over an interface panel belongs to that panel and does not
move the camera. In an orthographic view the wheel changes the scale without
moving the eye, exactly as swapping projections describes.

A two-finger pinch on a trackpad does the same thing, towards the same place,
and is the same operation underneath rather than a second kind of zoom: spread
to come closer, close to go away. It is anchored on the pointer exactly as the
wheel is, holds the same target-plane point with the same limitation about
surfaces nearer or further than it, obeys the same limits, and belongs to a
panel when it happens over one. The one difference is the unit: a pinch carries
a magnification delta, where a wheel counts notches. A phase event carrying no
magnification does nothing at all, including to a mouse drag that happens to be
in progress. Winit documents its dedicated pinch event on macOS and iOS;
FerriteCAD claims the desktop behaviour only on macOS. No particular mapping of
pinch gestures is claimed for Windows or Linux input stacks.

Two fingers turning against each other tilt the horizon: the view rolls about
the direction it is already looking in, the way the fingers went, so a part can
be brought square with the screen before it is read off. Nothing changes except
which way is up. The eye stays where it is, so does what you are looking at, and
so do the distance between them, the apparent scale and the projection. That is
what makes this different from dragging to orbit, which swings the eye around
the model about the world's up axis and deliberately levels the horizon again.
Framing keeps whatever tilt you have set. To put the horizon back, ask for a
named view or simply orbit. Like the pinch, this is a gesture winit documents
for macOS and iOS, and the desktop behaviour is claimed only on macOS.

A two-finger double tap magnifies, and a second one puts the view back exactly
where it was. What it magnifies is stated rather than guessed at: whatever is
selected, if anything selected is drawn, and otherwise everything currently
visible in the picture, which is not the same as everything in the file because
hidden parts are not on screen to be looked at. It is deliberately not the thing
under the pointer: the gesture carries no position and no geometry, and nothing
here reads the depth of a pixel to find out what is behind it. With nothing
selected and nothing visible there is nothing to look at, and the gesture does
nothing at all.

One level and no more. The tap that goes back uses up the way back, so the next
one magnifies afresh. Moving the view yourself in between, by any means at all,
gives up the way back rather than surprising you with it later: an orbit, a
drag, a wheel, a pinch, a turn, a named view, a change of projection, either
kind of framing and resizing the window all count. Opening a document gives it
up too, because a view of the document you just closed is not a view of this
one. There is no button and no shortcut, and nothing about it is remembered
after the application closes. Like the other trackpad gestures, this one is
delivered by macOS.

The model is drawn with the boundary of every face on it: a one-pixel line
wherever a face stops, whether against the empty background or against the next
face along. Without them a shaded body is a silhouette with shading inside it,
and two faces that meet at a shallow angle, or that lie in the same plane and
share a colour, are impossible to tell apart. The lines are drawn in whatever
contrasts with the surface beside them, so they are visible on a black part and
on a white one, they are hidden by anything nearer, and they follow the model
through every camera and both projections because they are drawn from the same
vertices through the same matrix as the surface itself.

What is drawn as ordinary linework is the boundary of the tessellation, and
that is worth stating plainly: it is where the triangles of one face end, not
a curve read from the original geometry. The seams inside a face, where one
triangle meets the next, are deliberately not drawn. That linework does not
invent a selectable object of its own. A separate topological-edge identity can
answer where the rendered samples agree, and a click selects it only when the
document has an exact durable name for it; otherwise the click falls through to
the named face or the part beneath it. Hiding a part takes both its surface and
its linework with it.

Moving the pointer over the model marks the most particular answer the picture
has: a coherent topological edge first, otherwise the face under it, otherwise
the whole definition. The same edge, face or definition is marked wherever
that definition appears. Moving over a row of the list marks that whole
definition everywhere. Neither route chooses anything, so geometry can be
found before anything is selected, and what is already selected keeps its
appearance while you look around it. A click chooses the most particular
durable answer the document has: an exact named native edge first, otherwise a
named native face, otherwise its definition. Opening another document is the
only editing operation there is.

## Which sketch solver this build has

```sh
cargo run -p ferritecad-app --bin ferritecad-viewer -- --solver-info
```

Answers and exits. It opens no window, reads no document and starts neither
Open CASCADE nor a graphics device, so it can be asked on a machine that has
neither a display nor a GPU.

An ordinary build has no sketch solver in it. planegcs is LGPL-2.0-or-later,
is built separately and is off by default, so the answer is a typed refusal
and the exit code is `3`. A build that linked it exits `0` and prints what the
loaded library says it is, which is the library's own answer and not a string
this program carries. A command line this viewer cannot act on still exits
`2`, as it always did.

This says which component is loaded. It does not say that sketches are
constrained: no document stores a constraint, no feature reads one, and no
release packages planegcs. Building and replacing the library is
[docs/build-planegcs.md](docs/build-planegcs.md).

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
