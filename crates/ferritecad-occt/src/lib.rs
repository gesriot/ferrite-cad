// SPDX-License-Identifier: MIT
//! Open CASCADE behind the FerriteCAD geometry contract.
//!
//! This crate is the first real implementation of
//! [`GeometryKernel`][ferritecad_kernel::GeometryKernel]. Everything above it
//! — the cold evaluator, the topology layer, eventually the interface — talks
//! to the trait and never to this type, which is what makes the kernel
//! replaceable rather than load-bearing.
//!
//! # Layers
//!
//! The C++ sources in `crates/ferritecad-occt-bridge` are the only place Open
//! CASCADE headers appear. They expose a flat `extern "C"` ABI: opaque
//! handles, status codes, caller-owned buffers, every function `noexcept`.
//! `ffi` is the only module in the workspace permitted `unsafe`, and it turns
//! that ABI into ordinary Rust results. `kernel` is safe code that implements
//! the trait.
//!
//! # Building without Open CASCADE
//!
//! OCCT cannot be fetched by cargo, so a checkout on a machine without it must
//! still build. When the build script cannot compile the bridge, this crate
//! compiles to a stub whose constructor explains itself, and the build prints
//! a warning. Set `FERRITECAD_REQUIRE_OCCT=1` to make that a build failure
//! instead — the pin workflow does, because a run whose purpose is to prove
//! the adapter works must not pass by skipping it.
//!
//! Use [`is_available`] to tell which build this is.

#[cfg(occt)]
mod ffi;
#[cfg(occt)]
mod kernel;
#[cfg(not(occt))]
mod unavailable;

#[cfg(occt)]
pub use kernel::OcctKernel;
#[cfg(not(occt))]
pub use unavailable::OcctKernel;

/// Whether this build has Open CASCADE compiled in.
///
/// A test that needs real geometry should skip rather than fail when this is
/// false: the absence of a kernel is a build configuration, not a defect. A
/// build that must have one sets `FERRITECAD_REQUIRE_OCCT=1` and never reaches
/// this question.
pub const fn is_available() -> bool {
    cfg!(occt)
}
