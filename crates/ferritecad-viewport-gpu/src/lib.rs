// SPDX-License-Identifier: MIT
//! Drawing a [`RenderSnapshot`] on a device, off screen.
//!
//! No window and no interface. This renders into textures it owns, reads them
//! back, and hands over a [`Frame`]. That is enough to prove a snapshot really
//! is drawable and that its definition, face and topological-edge targets
//! answer for the geometry that was drawn, which a window would otherwise be
//! the first to discover.
//!
//! # A frame keeps the snapshot it was drawn from
//!
//! [`Frame`] holds the very [`RenderSnapshot`] that produced it, and
//! [`Frame::pick_at`] resolves against that one. A pick buffer read against a
//! newer snapshot would land on whichever definition now occupies the number,
//! which is a wrong answer that looks like a right one – and exactly the shape
//! of mistake the rest of this project spends its effort refusing. Here it is
//! not refused but made impossible: the frame and the snapshot it belongs to
//! cannot be separated.
//!
//! # What belongs to what
//!
//! Geometry is uploaded once, by [`Renderer::prepare`], and stays on the
//! device. Drawing the result again writes a matrix and runs a pass; the
//! vertices are not touched. So the three things a viewport keeps live in
//! three places, each owned by whatever it actually belongs to: the model's
//! meaning in a [`RenderSnapshot`], its buffers in a [`PreparedSnapshot`], and
//! one reading of it in a [`Frame`].
//!
//! A [`PreparedSnapshot`] belongs to the renderer that prepared it, and another
//! refuses to draw it rather than reaching into a different device's memory –
//! the same arrangement the kernel uses for shape handles and sessions, for the
//! same reason. A [`Frame`] keeps its own snapshot, so a frame drawn before the
//! model changed still answers about the model it drew.
//!
//! # A window is where the operating system can interfere
//!
//! A surface is the one part of this that something else can take away:
//! windows are resized, minimised, occluded, dragged between displays, and
//! lost outright when a device resets. [`WindowSurface`] owns that lifecycle,
//! and what to do about each answer a surface gives is written as a function
//! over that answer alone, so it can be read and tested without a window –
//! see [`recovery_for`] and [`usable_size`].
//!
//! Opening a device for a window is also a different question from opening one
//! at all. [`Renderer::for_surface`] asks for an adapter that can present to
//! *that* surface; [`Renderer::new`] asks for any adapter that can compute.
//! A machine may hold several, and the first that can compute is not always
//! one connected to the display the window is on.
//!
//! # A frame is composed, not drawn once
//!
//! A surface hands out one texture per frame. A window that shows a model with
//! an interface over it therefore has to share that one texture: the model
//! goes in, the interface goes on top, and only then is the whole thing
//! published. [`WindowSurface::begin`] is that seam. An overlay that acquired
//! its own texture would be asking for a second frame, and one that ran after
//! the frame was presented would be drawing into something already on screen.
//!
//! [`WindowSurface::present`] remains the whole of a frame when there is
//! nothing above the model, and is built from the same seam so both paths
//! acquire and reconfigure identically.
//!
//! # Without a device
//!
//! [`Renderer::new`] fails when no adapter is available, which is an ordinary
//! condition on a headless machine rather than a defect. Callers are expected
//! to skip; nothing in the shipped product depends on a GPU being present.

mod renderer;
mod surface;

pub use renderer::{
    COLOUR_FORMAT, DEPTH_FORMAT, EDGE_FORMAT, FACE_FORMAT, Frame, Hit, PICK_FORMAT,
    PreparedSnapshot, Renderer, RendererId, VERTEX_FORMAT, VERTEX_PICK_RADIUS_PIXELS,
};
pub use surface::{
    ComposedSurfaceFrame, Presented, SurfaceFrame, SurfaceRecovery, WindowSurface, recovery_for,
    usable_size,
};
