// SPDX-License-Identifier: MIT
//
// A reference grid on the world's XY plane, drawn from nothing but a vertex
// number.
//
// There is no vertex buffer. Every line is derived from `vertex_index` and the
// spacing in the uniform below, so changing zoom changes one number rather
// than uploading geometry, and nothing about the model's buffers is touched to
// draw a backdrop. The lines are in world space and go through the same
// projection the model does: they move when the camera pans, foreshorten when
// it orbits, and stay on the plane rather than on the screen.

struct Grid {
    view_projection: mat4x4<f32>,
    // Between neighbouring lines, in the same millimetres as everything else.
    minor: f32,
    // Between the heavier ones. A whole multiple of `minor`, so a major line
    // is a minor line drawn more strongly rather than a line of its own.
    major: f32,
    // How far the grid runs from the origin along each axis.
    extent: f32,
    // Lines each side of the origin. The count is fixed, so what a zoom
    // changes is how much world the same lines cover.
    half_lines: u32,
};

@group(0) @binding(0) var<uniform> grid: Grid;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    // Where this line sits on the axis it crosses, which is what decides
    // whether it is an axis, a major line or a minor one.
    @location(0) coordinate: f32,
    // 0 for a line of constant x, 1 for a line of constant y.
    @location(1) along: f32,
};

@vertex
fn vertex_main(@builtin(vertex_index) vertex: u32) -> VertexOut {
    let per_axis = 2u * grid.half_lines + 1u;
    let line = vertex / 2u;
    let far_end = (vertex % 2u) == 1u;

    var position = vec3<f32>(0.0, 0.0, 0.0);
    var coordinate = 0.0;
    var along = 0.0;
    if (line < per_axis) {
        // Constant x, running along y.
        coordinate = (f32(line) - f32(grid.half_lines)) * grid.minor;
        position = vec3<f32>(coordinate, select(-grid.extent, grid.extent, far_end), 0.0);
    } else {
        coordinate = (f32(line - per_axis) - f32(grid.half_lines)) * grid.minor;
        position = vec3<f32>(select(-grid.extent, grid.extent, far_end), coordinate, 0.0);
        along = 1.0;
    }

    var out: VertexOut;
    out.clip = grid.view_projection * vec4<f32>(position, 1.0);
    out.coordinate = coordinate;
    out.along = along;
    return out;
}

struct FragmentOut {
    @location(0) colour: vec4<f32>,
    @location(1) pick: u32,
    @location(2) face: u32,
};

@fragment
fn fragment_main(in: VertexOut) -> FragmentOut {
    // A quarter of a minor step is comfortably inside the rounding of the
    // multiplication that produced the coordinate, and far from the next line.
    let tolerance = grid.minor * 0.25;

    var colour = vec3<f32>(0.22, 0.22, 0.24);
    if (abs(in.coordinate) < tolerance) {
        // An axis. Red along x and green along y, which is the convention a
        // person coming from any other CAD program already has.
        if (in.along > 0.5) {
            colour = vec3<f32>(0.62, 0.20, 0.20);
        } else {
            colour = vec3<f32>(0.20, 0.55, 0.24);
        }
    } else if (abs(round(in.coordinate / grid.major) * grid.major - in.coordinate) < tolerance) {
        colour = vec3<f32>(0.34, 0.34, 0.37);
    }

    var out: FragmentOut;
    out.colour = vec4<f32>(colour, 1.0);
    // Never a definition. A grid line is a thing to look at and not a thing to
    // choose, so clicking one is clicking the background, and this is where
    // that is true rather than somewhere that could forget it.
    out.pick = 0u;
    // And never a face, for the same reason: there is no surface here to name.
    out.face = 0u;
    return out;
}
