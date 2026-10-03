// SPDX-License-Identifier: MIT
//! Route AppKit termination (Cmd+Q, Quit menu and Dock Quit) through the same
//! document guard as closing the window. Winit 0.30 owns the app delegate but
//! implements only the did-launch/will-terminate notifications: the latter is
//! too late to keep an unsaved document open.
//!
//! Add the optional NSApplicationDelegate decision method, without replacing
//! the delegate or any winit method. Decline AppKit's immediate termination and
//! queue a normal event instead. The event loop then owns Save/Discard/Cancel
//! and its ordinary exit/worker cleanup. No document or UI borrow crosses the
//! Objective-C callback, including when AppKit is running a modal dialog.

// A single small, macOS-only runtime boundary. No Objective-C object is retained,
// released, subclassed or otherwise modified; only a previously absent method is
// registered. The SDK declares NSApplicationTerminateReply as NSUInteger (usize).
#![allow(unsafe_code)]

use std::ffi::{c_char, c_void};
use std::sync::OnceLock;

use ferritecad_types::{CadError, Result};
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

static EVENTS: OnceLock<EventLoopProxy<AppEvent>> = OnceLock::new();

type TerminateMethod = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> usize;

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn object_getClass(object: *mut c_void) -> *mut c_void;
    fn class_getInstanceMethod(class: *mut c_void, selector: *mut c_void) -> *mut c_void;
    fn class_addMethod(
        class: *mut c_void,
        selector: *mut c_void,
        implementation: TerminateMethod,
        encoding: *const c_char,
    ) -> bool;
    // This declaration is used only for the two no-argument, object-returning
    // selectors below; objc_msgSend's ABI is specialised to the sent method.
    #[link_name = "objc_msgSend"]
    fn send_object(receiver: *mut c_void, selector: *mut c_void) -> *mut c_void;
}

/// Called on the main thread immediately after winit constructs its event loop
/// and installs its application delegate, before any window is opened.
pub(crate) fn install(events: EventLoopProxy<AppEvent>) -> Result<()> {
    let failure = || CadError::rendering("could not install the macOS unsaved-document Quit guard");
    // SAFETY: The event-loop constructor guarantees the main thread and a live
    // AppKit application. The selectors are standard object-returning methods;
    // every returned pointer is checked before use. The registered implementation
    // and type encoding match the SDK's applicationShouldTerminate: declaration.
    unsafe {
        let app_class = objc_getClass(c"NSApplication".as_ptr());
        if app_class.is_null() {
            return Err(failure());
        }
        let app = send_object(app_class, sel_registerName(c"sharedApplication".as_ptr()));
        if app.is_null() {
            return Err(failure());
        }
        let delegate = send_object(app, sel_registerName(c"delegate".as_ptr()));
        if delegate.is_null() {
            return Err(failure());
        }
        let class = object_getClass(delegate);
        let selector = sel_registerName(c"applicationShouldTerminate:".as_ptr());
        if class.is_null() || !class_getInstanceMethod(class, selector).is_null() {
            // Never overwrite a policy introduced by a future winit version.
            return Err(failure());
        }
        EVENTS.set(events).map_err(|_| failure())?;
        if !class_addMethod(class, selector, should_terminate, c"Q@:@".as_ptr()) {
            return Err(failure());
        }
    }
    Ok(())
}

unsafe extern "C" fn should_terminate(
    _delegate: *mut c_void,
    _selector: *mut c_void,
    _application: *mut c_void,
) -> usize {
    if let Some(events) = EVENTS.get() {
        let _ = events.send_event(AppEvent::QuitRequested);
    }
    // NSTerminateCancel. An unavailable event loop is never permission to drop
    // unsaved data; a live loop makes the guarded decision through its own exit.
    0
}
