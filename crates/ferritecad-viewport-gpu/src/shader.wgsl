// SPDX-License-Identifier: MIT
//
// The smallest shader that draws a model and says what was clicked.
//
// Two colour attachments, written in one pass. Drawing the picture and drawing
// the identities separately would mean two passes that could disagree about
// which triangle won the depth test, and the pick would then be right about a
// frame nobody saw.

struct Globals {
    view_projection: mat4x4<f32>,
    // Which definition is chosen, or zero for none. Compared against each
    // draw's own identity, which is what makes every placement of one
    // definition light up together: they carry the same number, so this is one
    // comparison rather than a list the renderer would have to keep in step.
    selected: u32,
    // Which definition the pointer is over, or zero. A question rather than a
    // decision, and drawn differently so the two can be told apart.
    hovered: u32,
    padding_0: u32,
    padding_1: u32,
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
};

@fragment
fn fragment_main(in: VertexOut) -> FragmentOut {
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

    // Lifted towards white rather than replaced by a colour of its own: what
    // is chosen must still look like the material it is, and a part that
    // turned orange would hide whatever the file said about it.
    var tint = draw.colour.rgb;
    if (globals.selected != 0u && draw.pick == globals.selected) {
        // A choice already made. Kept as it was, and stronger than the
        // question below, so pointing at something never looks like having
        // chosen it.
        tint = mix(tint, vec3<f32>(1.0, 1.0, 1.0), 0.55);
    } else if (globals.hovered != 0u && draw.pick == globals.hovered) {
        // Merely under the pointer: lifted enough to find, far enough from
        // the selection to be another thing.
        tint = mix(tint, vec3<f32>(1.0, 1.0, 1.0), 0.22);
    }

    var out: FragmentOut;
    out.colour = vec4<f32>(tint * lambert, draw.colour.a);
    out.pick = draw.pick;
    return out;
}
