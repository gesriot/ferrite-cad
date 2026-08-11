// SPDX-License-Identifier: MIT
//! Types shared by every FerriteCAD crate.
//!
//! This crate deliberately knows nothing about geometry kernels, storage or
//! user interface. It defines the vocabulary the rest of the system agrees on:
//! identifiers, the internal unit system, tolerances, the error taxonomy and
//! the canonical hashing rules that make rebuilds deterministic.
//!
//! # Internal units
//!
//! Everything inside the system is millimetres and radians. Units exist for
//! display and for parsing user input; they are converted at the boundary and
//! never stored in a feature payload.

mod error;
mod hash;
mod ids;
mod tolerance;
mod transform;
mod units;

pub use error::{BoxError, CadError, ErrorKind, Result};
pub use hash::{CanonicalHasher, ContentHash, normalize_f64};
pub use ids::{DocumentId, FeatureId, ImportedSourceId, ObjectId, StableEntityId};
pub use tolerance::Tolerance;
pub use transform::{Point3, Transform, Vec3};
pub use units::{Dimension, Quantity, Unit};
