// SPDX-License-Identifier: MIT
//! Deciding what to rebuild, and in what order.
//!
//! The core of this crate answers two questions: given that some objects
//! changed, *which* objects are now stale, and *in what order* may they be
//! recomputed. Those parts spawn no threads and know nothing about any
//! kernel — they are pure functions over identifiers and edges, which is what
//! makes them testable without a document on disk and without Open CASCADE
//! present at all. Around them sit the two things that need a kernel to mean
//! anything: [`rebuild_cold`], which runs a plan, and
//! [`store_extrude_archive`], which writes one feature's result somewhere it
//! can be found again. Neither owns a kernel session or a cache sidecar; both
//! are handed one.
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

mod cache;
mod cold;
mod convert;
mod dirty;
mod document_graph;
mod plan;
mod solve;

pub use cache::{extrude_archive_key, load_extrude_archive, store_extrude_archive};
pub use cold::{CacheEvent, CacheOutcome, RebuildResult, rebuild_cached, rebuild_cold};
pub use convert::{extrude_request, plane_from_datum, profile_from_sketch};
pub use dirty::{DependentIndex, dirty_set};
pub use document_graph::DocumentGraph;
pub use plan::{RebuildPlan, plan_full_rebuild, plan_rebuild};
