// SPDX-License-Identifier: MIT
//
// The smallest shader that draws a model and says what was clicked.
//
// Colour, definition and face, written in one pass. Drawing the picture and
// drawing the identities separately would mean two passes that could disagree
// about which triangle won the depth test, and the pick would then be right
// about a frame nobody saw.

// Which triangle a pixel came from. Declared here and required of the device
// in `Renderer::on`, because a face is a property of a triangle and nothing
// else in a pipeline knows one triangle from the next.
enable primitive_index;

struct Globals {
    view_projection: mat4x4<f32>,
    // Which definition is chosen, or zero for none. Compared against each
    // draw's own identity, which is what makes every placement of one
    // definition light up together: they carry the same number, so this is one
    // comparison rather than a list the renderer would have to keep in step.
    selected: u32,
    // Which face the pointer is over, or zero. A face of the picture rather
    // than of a placement, so the same face lights up wherever its definition
    // appears.
    hovered_face: u32,
    // Which definition the pointer is over, or zero. A question rather than a
    // decision, and drawn differently so the two can be told apart.
    hovered: u32,
    padding_0: u32,
};

struct Draw {
    transform: mat4x4<f32>,
    colour: vec4<f32>,
    // What a click on this draw identifies: its definition, never the
    // placement. The renderer is handed this value and has no way to compute
    // one, which is where that guarantee is kept.
    pick: u32,
    // Where this draw's mesh starts in the picture's table of faces.
    first_triangle: u32,
    // Two scalars rather than a vec3<u32>. A vec3 has sixteen-byte
    // alignment, which would push this struct to 112 bytes while the Rust type
    // that fills it is 96, and the two must agree exactly or the binding is
    // rejected. Scalars keep both at 96.
    padding_0: u32,
    padding_1: u32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<uniform> draw: Draw;
// One identity per triangle of the whole picture, uploaded with the geometry.
// A face is a lookup rather than a vertex attribute, so nothing is duplicated
// and no draw is split to say which face a triangle belongs to.
@group(0) @binding(2) var<storage, read> faces: array<u32>;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
};

@vertex
fn vertex_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
) -> VertexOut {
    var out: VertexOut;
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

@fragment
fn fragment_main(in: VertexOut, @builtin(primitive_index) triangle: u32) -> FragmentOut {
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

    // Shifted in brightness rather than replaced by a colour of its own: what
    // is marked must still look like the material it is, and a part that
    // turned orange would hide whatever the file said about it.
    // Which face of the picture this pixel came from. The triangle number is
    // counted within the draw, so the draw says where its mesh begins.
    let face = faces[draw.first_triangle + triangle];

    var tint = draw.colour.rgb;
    if (globals.selected != 0u && draw.pick == globals.selected) {
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

    var out: FragmentOut;
    out.colour = vec4<f32>(tint * lambert, draw.colour.a);
    out.pick = draw.pick;
    out.face = face;
    return out;
}
