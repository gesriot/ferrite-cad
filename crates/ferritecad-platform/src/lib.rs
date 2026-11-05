// SPDX-License-Identifier: MIT
//! Platform FFI for reclaiming unused process memory.

/// Ask the system allocator to return unused pages after a revision is released.
/// This best-effort operation leaves live allocations intact.
pub fn release_unused_memory() {
    #[cfg(target_os = "macos")]
    macos::release_unused_memory();
}

#[cfg(target_os = "macos")]
mod macos {
    #![allow(unsafe_code, reason = "the platform allocator FFI boundary")]

    unsafe extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }

    pub(super) fn release_unused_memory() {
        // A null zone asks all process zones to release their unused pages.
        // Zero requests maximal relief, and no caller-owned pointer crosses the ABI.
        unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
    }
}
