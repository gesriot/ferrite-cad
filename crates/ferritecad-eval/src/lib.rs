//! Deciding what to rebuild, and in what order.
//!
//! This crate answers two questions and nothing else: given that some objects
//! changed, *which* objects are now stale, and *in what order* may they be
//! recomputed. It computes no geometry, holds no cache, spawns no threads and
//! knows nothing about any kernel — everything here is a pure function over
//! identifiers and edges, which is what makes it testable without a document
//! on disk and without Open CASCADE present at all.
//!
//! # Determinism
//!
//! Every result is fully determined by the graph's content, never by the order
//! it was handed over or by a hash seed. Two machines given the same document
//! produce the same plan, level for level. That is not tidiness: comparing
//! rebuild results across platforms is impossible if the schedule itself can
//! differ, and a non-deterministic schedule turns an intermittent kernel bug
//! into one nobody can reproduce.
//!
//! # What "dirty" means here
//!
//! Dirtiness travels along dependency edges in one direction only: if an object
//! changed, everything that reads it is stale too, transitively. A dependency
//! that did *not* change stays clean, and a stale object is allowed to depend on
//! it — that clean result is exactly what a cache is for. Independent branches
//! of the graph are untouched.

mod dirty;
mod document_graph;
mod plan;

pub use dirty::{DependentIndex, dirty_set};
pub use document_graph::DocumentGraph;
pub use plan::{RebuildPlan, plan_full_rebuild, plan_rebuild};
