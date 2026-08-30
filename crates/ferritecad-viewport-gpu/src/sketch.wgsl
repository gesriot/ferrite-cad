// SPDX-License-Identifier: MIT
//
// A document's drawings, in world space and at a width measured in pixels.
//
// Every position here is already in the world: the plane a sketch sits on, the
// angles of its arcs and the flag that says a curve only guides the drawing
// were all read at the boundary that knew about documents, and what arrives is
// two ends of a segment and how to colour it. So this file orbits, pans, zooms
// and rolls with the model for free, because it goes through exactly the
// matrix the model goes through and nothing else.
//
// # Why the width is applied here and not in the buffer
//
// A stroke has to stay readable, which means a fixed number of screen pixels
// rather than a length in millimetres: a width baked into the vertices would
// be a smear when zoomed in and invisible when zoomed out, and it would differ
// between the two projections. So each segment is uploaded once as its two
// ends, and this stage widens it after the projection - which is also what
// keeps a camera movement from touching a single byte of the buffer.
//
// # A point is a segment of no length
//
// The expansion runs along the segment and across it, and both offsets are
// given per vertex. A point sets both ends to the same place, the run is zero,
// the fallback direction is the screen's own x axis, and the four offsets draw
// a square around it. One primitive, one pipeline, and a point that cannot end
// up a different size from the strokes beside it.

struct Sketch {
    view_projection: mat4x4<f32>,
    // The frame's size in pixels, which is what turns a width in pixels into
    // an offset in clip space.
    viewport: vec2<f32>,
    padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> sketch: Sketch;

struct SketchOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) colour: vec3<f32>,
};

@vertex
fn vertex_sketch(
    // This vertex's own end of the segment.
    @location(0) here: vec3<f32>,
    // The other end, which is only ever read to work out which way the segment
    // runs on screen.
    @location(1) there: vec3<f32>,
    @location(2) colour: vec3<f32>,
    // Along the segment and across it, each -1 or +1. Along is what gives a
    // stroke square ends, which is also what closes the join between two
    // segments of one run without a second pass over the geometry.
    @location(3) offset: vec2<f32>,
    @location(4) half_width: f32,
) -> SketchOut {
    let near = sketch.view_projection * vec4<f32>(here, 1.0);
    let far = sketch.view_projection * vec4<f32>(there, 1.0);

    // Behind the eye a `w` of zero or less divides to nonsense, and the
    // direction is the only thing these divisions are for. Clamped, so a
    // segment reaching past the eye still has a well-defined width; the
    // vertex itself keeps its own `w` and is clipped by the rasteriser as any
    // other is.
    let near_w = max(near.w, 1.0e-6);
    let far_w = max(far.w, 1.0e-6);
    let half = sketch.viewport * 0.5;
    let at_here = vec2<f32>(near.x / near_w, near.y / near_w) * half;
    let at_there = vec2<f32>(far.x / far_w, far.y / far_w) * half;

    let run = at_there - at_here;
    let extent = length(run);
    // A point, or a segment whose two ends land on one pixel. The screen's own
    // x axis is as good a direction as any and, unlike normalising a zero, it
    // is a number.
    var along = vec2<f32>(1.0, 0.0);
    if (extent > 1.0e-6) {
        along = run / extent;
    }
    let across = vec2<f32>(-along.y, along.x);
    let moved = (along * offset.x + across * offset.y) * half_width;

    var out: SketchOut;
    out.colour = colour;
    out.clip = vec4<f32>(
        near.x + moved.x / half.x * near.w,
        near.y + moved.y / half.y * near.w,
        near.z,
        near.w,
    );
    return out;
}

// Colour and nothing else, whatever this is drawn into.
//
// One entry point for the window and the readback alike, because the pass it
// runs in has one colour attachment in both: a drawing writes no definition,
// no face, no edge and no vertex, so there is nothing here that could reach
// them even by mistake.
@fragment
fn fragment_sketch(in: SketchOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.colour, 1.0);
}
