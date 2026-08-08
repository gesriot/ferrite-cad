// SPDX-License-Identifier: MIT
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ferritecad_types::{CadError, Result, Tolerance};

/// Asks a running operation to stop.
///
/// Cheap to clone and safe to share, so the interface can hold one while a
/// worker holds another. Nothing here is tied to a particular kernel's
/// cancellation mechanism: an adapter polls this and translates.
///
/// Cancelling is a request, not a guarantee. An operation already inside a
/// kernel call finishes that call; what cancellation promises is that the
/// result is discarded rather than stored, so a cancelled rebuild leaves
/// neither the document nor the cache half-written.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Returns [`CadError::Cancelled`] if cancellation has been requested.
    ///
    /// Adapters call this between units of work. One error variant for every
    /// cancellation is what lets a caller tell "the user changed their mind"
    /// apart from "this geometry cannot be built", which deserve very
    /// different treatment in the interface.
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            return Err(CadError::Cancelled);
        }
        Ok(())
    }
}

/// Where an operation reports how far along it is.
///
/// Optional by design: an operation that reports nothing is merely quiet, never
/// broken. Fractions are clamped to `0.0..=1.0` so a badly behaved adapter
/// cannot drive a progress bar backwards or past the end.
#[derive(Clone, Default)]
pub struct ProgressSink {
    sink: Option<Arc<dyn Fn(f64) + Send + Sync>>,
}

impl ProgressSink {
    /// A sink that discards everything.
    pub fn silent() -> Self {
        Self::default()
    }

    pub fn new(sink: impl Fn(f64) + Send + Sync + 'static) -> Self {
        Self {
            sink: Some(Arc::new(sink)),
        }
    }

    /// Reports completion as a fraction of the whole.
    pub fn report(&self, fraction: f64) {
        if let Some(sink) = &self.sink {
            let clamped = if fraction.is_nan() {
                0.0
            } else {
                fraction.clamp(0.0, 1.0)
            };
            sink(clamped);
        }
    }

    pub fn is_silent(&self) -> bool {
        self.sink.is_none()
    }
}

impl fmt::Debug for ProgressSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgressSink")
            .field("silent", &self.is_silent())
            .finish()
    }
}

/// Everything an operation needs beyond its own arguments.
///
/// Tolerance travels here rather than being left to a kernel default, because
/// it is part of every cache key: the same inputs at a different tolerance are
/// a different result, not a reusable one.
#[derive(Debug, Clone, Default)]
pub struct OperationContext {
    tolerance: Tolerance,
    cancel: CancelToken,
    progress: ProgressSink,
}

impl OperationContext {
    /// A context at the given tolerance, uncancelled and silent.
    pub fn new(tolerance: Tolerance) -> Self {
        Self {
            tolerance,
            cancel: CancelToken::new(),
            progress: ProgressSink::silent(),
        }
    }

    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = cancel;
        self
    }

    pub fn with_progress(mut self, progress: ProgressSink) -> Self {
        self.progress = progress;
        self
    }

    pub fn tolerance(&self) -> Tolerance {
        self.tolerance
    }

    pub fn cancel(&self) -> &CancelToken {
        &self.cancel
    }

    pub fn progress(&self) -> &ProgressSink {
        &self.progress
    }

    /// Shorthand for [`CancelToken::check`].
    pub fn check_cancelled(&self) -> Result<()> {
        self.cancel.check()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn an_uncancelled_token_passes() {
        assert!(CancelToken::new().check().is_ok());
    }

    #[test]
    fn cancellation_produces_the_cancelled_variant() {
        let token = CancelToken::new();
        token.cancel();

        let err = token.check().expect_err("a cancelled token must refuse");
        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Cancellation);
        assert!(matches!(err, CadError::Cancelled));
    }

    #[test]
    fn a_clone_shares_the_cancellation() {
        let held_by_ui = CancelToken::new();
        let held_by_worker = held_by_ui.clone();

        held_by_ui.cancel();
        assert!(held_by_worker.is_cancelled());
    }

    #[test]
    fn a_silent_sink_accepts_reports_and_does_nothing() {
        let sink = ProgressSink::silent();
        assert!(sink.is_silent());
        sink.report(0.5);
    }

    #[test]
    fn progress_reaches_the_sink() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let sink = ProgressSink::new(move |fraction| {
            if let Ok(mut values) = recorder.lock() {
                values.push(fraction);
            }
        });

        sink.report(0.25);
        sink.report(0.75);

        let values = seen.lock().expect("no thread panicked while holding it");
        assert_eq!(*values, vec![0.25, 0.75]);
    }

    #[test]
    fn out_of_range_progress_is_clamped_rather_than_passed_on() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let sink = ProgressSink::new(move |fraction| {
            if let Ok(mut values) = recorder.lock() {
                values.push(fraction);
            }
        });

        sink.report(-1.0);
        sink.report(7.0);
        sink.report(f64::NAN);

        let values = seen.lock().expect("no thread panicked while holding it");
        assert_eq!(*values, vec![0.0, 1.0, 0.0]);
    }

    #[test]
    fn a_context_carries_its_tolerance() {
        let coarse = Tolerance::new(1e-3, 1e-6).expect("positive");
        let context = OperationContext::new(coarse);
        assert_eq!(context.tolerance(), coarse);
        assert!(context.check_cancelled().is_ok());
    }

    #[test]
    fn a_context_reports_cancellation_from_its_token() {
        let token = CancelToken::new();
        let context = OperationContext::new(Tolerance::default()).with_cancel(token.clone());

        assert!(context.check_cancelled().is_ok());
        token.cancel();
        assert!(matches!(
            context.check_cancelled(),
            Err(CadError::Cancelled)
        ));
    }
}
