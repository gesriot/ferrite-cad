// SPDX-License-Identifier: MIT
//
// The smallest shader that draws a model and says what was clicked.
//
// Colour, definition and face, written in one pass. Drawing the picture and
// drawing the identities separately would mean two passes that could disagree
// about which triangle won the depth test, and the pick would then be right
// about a frame nobody saw.

struct Globals {
    view_projection: mat4x4<f32>,
    // Which definition is chosen, or zero for none. Compared against each
    // draw's own identity, which is what makes every placement of one
    // definition light up together: they carry the same number, so this is one
    // comparison rather than a list the renderer would have to keep in step.
    selected: u32,
    // Which face is chosen, or zero. Never set beside `selected`: choosing a
    // face chooses that face and not the part it belongs to.
    selected_face: u32,
    // Which face the pointer is over, or zero. A face of the picture rather
    // than of a placement, so the same face lights up wherever its definition
    // appears.
    hovered_face: u32,
    // Which definition the pointer is over, or zero. A question rather than a
    // decision, and drawn differently so the two can be told apart.
    hovered: u32,
    // Which topological edge the pointer is over, or zero.
    hovered_edge: u32,
    // Three scalars of padding, spelled out. The matrix above gives this
    // struct sixteen-byte alignment, so WGSL rounds its size up to a multiple
    // of sixteen; without these the Rust type would be 84 bytes and this one
    // 96. Both are 96, and Rust asserts it.
    padding_0: u32,
    padding_1: u32,
    padding_2: u32,
};

struct Draw {
    transform: mat4x4<f32>,
    colour: vec4<f32>,
    // What a click on this draw identifies: its definition, never the
    // placement. The renderer is handed this value and has no way to compute
    // one, which is where that guarantee is kept.
    pick: u32,
    // Three scalars rather than a vec3<u32>. A vec3 has sixteen-byte
    // alignment, which would push this struct to 112 bytes while the Rust type
    // that fills it is 96, and the two must agree exactly or the binding is
    // rejected. Scalars keep both at 96.
    padding_0: u32,
    padding_1: u32,
    padding_2: u32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<uniform> draw: Draw;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    // Flat, because a face is a fact about the surface and not a quantity
    // that varies across it. Every vertex of a triangle carries the same
    // value, which the packer checks, so no interpolation could be right.
    @location(1) @interpolate(flat) face: u32,
};

@vertex
fn vertex_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) face: u32,
) -> VertexOut {
    var out: VertexOut;
    out.face = face;
    let world = draw.transform * vec4<f32>(position, 1.0);
    out.clip = globals.view_projection * world;
    // A normal follows the inverse transpose, not the transform itself. The
    // cofactor matrix is the inverse transpose multiplied by determinant;
    // normalisation below removes its magnitude, and the two-sided lighting
    // makes the determinant's sign immaterial. This form also stays defined
    // for a singular transform instead of dividing by zero.
    let x = draw.transform[0].xyz;
    let y = draw.transform[1].xyz;
    let z = draw.transform[2].xyz;
    let normal_matrix = mat3x3<f32>(cross(y, z), cross(z, x), cross(x, y));
    out.normal = normal_matrix * normal;
    return out;
}

struct FragmentOut {
    @location(0) colour: vec4<f32>,
    @location(1) pick: u32,
    @location(2) face: u32,
};

// Move away from the colour already on screen. Always lifting towards white
// makes a white part impossible to mark; always darkening has the same defect
// for black. The opposite half of the range preserves the material's hue while
// guaranteeing contrast for every displayable RGB value.
fn marked_colour(colour: vec3<f32>, strength: f32) -> vec3<f32> {
    let shown = clamp(colour, vec3<f32>(0.0), vec3<f32>(1.0));
    let luminance = dot(shown, vec3<f32>(0.2126, 0.7152, 0.0722));
    var endpoint = vec3<f32>(1.0);
    if (luminance > 0.5) {
        endpoint = vec3<f32>(0.0);
    }
    return mix(shown, endpoint, strength);
}

// What the model looks like at one pixel, given which face it came from.
//
// Shared by both entry points, because a window and a readback must agree
// about the picture down to the byte: they differ in what they record about
// it, never in what it looks like.
fn shade(in: VertexOut, face: u32) -> vec4<f32> {
    // A zero-length normal cannot be normalised, and normalize() of one is a
    // NaN that propagates into the colour attachment. Face the viewer instead.
    let length = dot(in.normal, in.normal);
    var normal = vec3<f32>(0.0, 0.0, 1.0);
    if (length > 1e-12) {
        normal = in.normal * inverseSqrt(length);
    }

    let to_light = normalize(vec3<f32>(0.3, -0.6, 0.7));
    // Two-sided: an imported assembly is not obliged to have consistent
    // winding, and a black facing is harder to diagnose than a lit one.
    let lambert = abs(dot(normal, to_light)) * 0.8 + 0.2;

    let tint = tint_of(face);

    return vec4<f32>(tint * lambert, draw.colour.a);
}

// The colour this draw shows for a face, before any light falls on it.
//
// Shifted in brightness rather than replaced by a colour of its own: what is
// marked must still look like the material it is, and a part that turned
// orange would hide whatever the file said about it.
//
// Shared with the linework, so a chosen part is chosen at its edges too and
// there is one statement of what marking means.
fn tint_of(face: u32) -> vec3<f32> {
    var tint = draw.colour.rgb;
    if (globals.selected_face != 0u && face == globals.selected_face) {
        // One chosen face, in every placement of its definition and nowhere
        // else. Further from the material than a chosen definition, because a
        // person has to be able to tell "this face" from "this part" without
        // consulting a panel.
        tint = marked_colour(tint, 0.82);
    } else if (globals.selected != 0u && draw.pick == globals.selected) {
        // A choice already made. Kept as it was, and stronger than the
        // questions below, so pointing at something never looks like having
        // chosen it.
        tint = marked_colour(tint, 0.55);
    } else if (globals.hovered_face != 0u && face == globals.hovered_face) {
        // One face under the pointer. Marked by its own identity rather than
        // its draw's, which is why the same face of a definition placed twice
        // is marked in both places and its neighbour in neither.
        tint = marked_colour(tint, 0.22);
    } else if (globals.hovered != 0u && draw.pick == globals.hovered) {
        // Merely under the pointer: shifted enough to find, far enough from
        // the selection to be another thing.
        tint = marked_colour(tint, 0.22);
    }
    return tint;
}

// The offscreen path: the picture and both facts about each pixel of it.
@fragment
fn fragment_main(in: VertexOut) -> FragmentOut {
    let face = in.face;
    var out: FragmentOut;
    out.colour = shade(in, face);
    out.pick = draw.pick;
    out.face = face;
    return out;
}

// A window's path: colour and nothing else.
//
// A second entry point rather than one that writes identities into targets a
// window does not have. Direct3D rejects that outright, and a shader whose
// output signature is wider than the pipeline it is compiled for is wrong even
// where a driver tolerates it.
@fragment
fn fragment_colour(in: VertexOut) -> @location(0) vec4<f32> {
    return shade(in, in.face);
}

// The ink a boundary is drawn in, for whatever the surface beside it shows.
//
// Taken from the shaded colour at this very pixel and carried most of the way
// to the opposite end of the range. Three things follow, and all three are
// wanted. Linework on a black part is visible, because the ink goes towards
// white there and towards black on a light one. It stays visible on a face
// turned away from the light, because it is measured against that face's own
// shading rather than against the material. And it moves whenever the fill
// moves, so choosing or pointing at a part changes it everywhere the part is
// drawn, edges included, rather than leaving a few pixels behind looking
// exactly as they did.
//
// All the way rather than most of it: a boundary is a line, and a line that
// shaded itself would be a soft edge. What follows from that is stated where
// it matters: the ink of a chosen part looks like the ink of an unchosen one
// wherever both fills fall on the same side of the range, so it is the
// surface, not the boundary, that shows a part has been chosen.
fn ink(in: VertexOut, face: u32) -> vec4<f32> {
    let lit = shade(in, face);
    return vec4<f32>(marked_colour(lit.rgb, 1.0), lit.a);
}

// Where a face stops, on the offscreen path.
//
// The identities are returned but the pipeline writes neither: a line is
// something to look at and not something to click, and the pixel underneath
// must still answer with the face it belongs to.
@fragment
fn fragment_line(in: VertexOut) -> FragmentOut {
    var out: FragmentOut;
    out.colour = ink(in, in.face);
    out.pick = 0u;
    out.face = 0u;
    return out;
}

// The same line on a window, which has only a colour to write.
@fragment
fn fragment_line_colour(in: VertexOut) -> @location(0) vec4<f32> {
    return ink(in, in.face);
}

// Which topological edge of the model a pixel is on.
//
// Its own vertex stream, and it has to be. A vertex buffer parallel to the
// positions could carry one edge identity per vertex, exactly as the faces do,
// and that works for faces only because a tessellation gives each face its own
// nodes. Edges are not like that: one corner of a box is an end of three
// different edges, so a single value per position cannot say which of them a
// line belongs to. So every segment is expanded into two vertices of its own,
// each carrying its edge's identity, and a position that several edges meet at
// simply appears several times.
//
// The same matrices as everything else. A pass that projected differently
// would put its answer somewhere other than the picture it is answering about.
struct EdgeOut {
    @builtin(position) clip: vec4<f32>,
    // Flat: an edge identity is a fact about the whole segment, and there is
    // nothing between two of them to interpolate.
    @location(0) @interpolate(flat) edge: u32,
};

@vertex
fn vertex_edge(
    @location(0) position: vec3<f32>,
    @location(1) edge: u32,
) -> EdgeOut {
    var out: EdgeOut;
    out.edge = edge;
    out.clip = globals.view_projection * (draw.transform * vec4<f32>(position, 1.0));
    return out;
}

// One target and one integer. No colour, no definition and no face: this pass
// has none of those attachments, so it cannot write them by mistake.
@fragment
fn fragment_edge(in: EdgeOut) -> @location(0) u32 {
    return in.edge;
}

// The one topological edge under the pointer, drawn over the picture.
//
// The same expanded vertex stream the identity pass uses, and the same
// matrices: a highlight computed through arithmetic of its own would come
// away from the line it is meant to mark as soon as the camera moved.
//
// Every segment of the edge is drawn, from both of the faces that meet at it
// and in every placement of the definition, because an identity belongs to the
// edge and not to one side of it or one occurrence of it. Everything else is
// discarded, so pointing at one edge changes that edge and nothing around it.
struct EdgeMarkOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) edge: u32,
};

@vertex
fn vertex_edge_mark(
    @location(0) position: vec3<f32>,
    @location(1) edge: u32,
) -> EdgeMarkOut {
    var out: EdgeMarkOut;
    out.edge = edge;
    out.clip = globals.view_projection * (draw.transform * vec4<f32>(position, 1.0));
    return out;
}

// What the marked edge is drawn in.
//
// Two things at once, and both are needed. The base is the end of the range
// opposite this part's material, which is what guarantees the mark is visible
// on a part of any colour: towards white on a very dark one and towards black
// on a very light one. It is then carried half way to a fixed warm accent,
// which is what makes it a different thing to look at rather than a brighter
// version of something already there. Every other line in the picture is
// achromatic by construction, and a choice or a question about a face or a
// part moves the fill rather than the line, so this is the only place where a
// line has a hue.
//
// Measured against the material rather than against the shading at this pixel,
// and that is a real simplification: the expanded edge stream carries no
// normal, because the identity pass it is shared with has no use for one, and
// a stream with normals would be a second copy of the same geometry. The half
// step to the accent is what keeps the mark legible on a face turned away from
// the light, where the shading is darker than the material.
//
// Only the samples of this edge change. The face it lies on and the part that
// face belongs to keep exactly the colour they had.
@fragment
fn fragment_edge_mark(in: EdgeMarkOut) -> @location(0) vec4<f32> {
    if (globals.hovered_edge == 0u || in.edge != globals.hovered_edge) {
        discard;
    }
    let ink = marked_colour(draw.colour.rgb, 1.0);
    let accent = vec3<f32>(1.0, 0.55, 0.1);
    return vec4<f32>(mix(ink, accent, 0.5), draw.colour.a);
}
