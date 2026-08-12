// SPDX-License-Identifier: MIT
//! Drawing a [`RenderSnapshot`] on a device, off screen.
//!
//! No window and no interface. This renders into textures it owns, reads them
//! back, and hands over a [`Frame`]. That is enough to prove a snapshot really
//! is drawable and that a pick really does come back as the definition that was
//! drawn, which are the two things a window would otherwise be the first to
//! discover.
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
//! # Without a device
//!
//! [`Renderer::new`] fails when no adapter is available, which is an ordinary
//! condition on a headless machine rather than a defect. Callers are expected
//! to skip; nothing in the shipped product depends on a GPU being present.

mod renderer;

pub use renderer::{
    COLOUR_FORMAT, DEPTH_FORMAT, Frame, PICK_FORMAT, PreparedSnapshot, Renderer, RendererId,
};
