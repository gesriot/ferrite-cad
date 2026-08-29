// SPDX-License-Identifier: MIT
use thiserror::Error;

/// Boxed source error, used to keep the original cause reachable through
/// [`std::error::Error::source`] instead of flattening it into a string.
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub type Result<T, E = CadError> = std::result::Result<T, E>;

/// Coarse classification of a failure.
///
/// The user interface groups and routes errors by this value, so it is part of
/// the contract: adding a [`CadError`] variant means deciding which class it
/// belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Malformed or out-of-range input, including a corrupt document.
    Input,
    /// A sketch or feature constraint system that cannot be satisfied.
    Constraint,
    /// A topology reference that could not be resolved to real geometry.
    Topology,
    /// The geometry kernel refused or failed to produce a result.
    Kernel,
    /// The graphics device or renderer failed to produce a frame.
    Rendering,
    /// Storage or filesystem failure.
    Io,
    /// The operation was cancelled before it produced a result.
    Cancellation,
    /// The request is well-formed but this build cannot serve it.
    Unsupported,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Constraint => "constraint",
            Self::Topology => "topology",
            Self::Kernel => "kernel",
            Self::Rendering => "rendering",
            Self::Io => "io",
            Self::Cancellation => "cancellation",
            Self::Unsupported => "unsupported",
        }
    }
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The single error type crossing FerriteCAD's public boundaries.
///
/// A failed operation is always better than a silently wrong one: nothing in
/// this crate encourages a caller to continue past an error, and no variant
/// discards its cause.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CadError {
    #[error("invalid input: {message}")]
    Input {
        message: String,
        #[source]
        source: Option<BoxError>,
    },

    #[error("constraint problem: {message}")]
    Constraint {
        message: String,
        /// What the refusal was about, when the refusal is about something a
        /// caller can act on rather than only read.
        ///
        /// Optional for the same reason [`Self::Input`]'s is: most constraint
        /// failures are a sentence and nothing more. The ones that are not
        /// carry a value here, and a caller reads it by asking the crate that
        /// owns that value whether this is one of its own — never by reading
        /// the message, which is written for a person.
        #[source]
        source: Option<BoxError>,
    },

    #[error("unresolved topology reference: {message}")]
    Topology { message: String },

    #[error("geometry kernel failure: {message}")]
    Kernel {
        message: String,
        #[source]
        source: Option<BoxError>,
    },

    #[error("rendering failure: {message}")]
    Rendering {
        message: String,
        #[source]
        source: Option<BoxError>,
    },

    #[error("{context}")]
    Io {
        context: String,
        #[source]
        source: BoxError,
    },

    #[error("operation cancelled")]
    Cancelled,

    #[error("unsupported: {message}")]
    Unsupported { message: String },
}

impl CadError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Input { .. } => ErrorKind::Input,
            Self::Constraint { .. } => ErrorKind::Constraint,
            Self::Topology { .. } => ErrorKind::Topology,
            Self::Kernel { .. } => ErrorKind::Kernel,
            Self::Rendering { .. } => ErrorKind::Rendering,
            Self::Io { .. } => ErrorKind::Io,
            Self::Cancelled => ErrorKind::Cancellation,
            Self::Unsupported { .. } => ErrorKind::Unsupported,
        }
    }

    pub fn input(message: impl Into<String>) -> Self {
        Self::Input {
            message: message.into(),
            source: None,
        }
    }

    pub fn input_because(message: impl Into<String>, source: impl Into<BoxError>) -> Self {
        Self::Input {
            message: message.into(),
            source: Some(source.into()),
        }
    }

    pub fn constraint(message: impl Into<String>) -> Self {
        Self::Constraint {
            message: message.into(),
            source: None,
        }
    }

    /// A constraint failure that carries what it was about.
    ///
    /// The message stays the whole of what a person is shown; `source` is what
    /// a caller that knows the type can ask for. Nothing here inspects it, and
    /// no other variant gains one by association.
    pub fn constraint_because(message: impl Into<String>, source: impl Into<BoxError>) -> Self {
        Self::Constraint {
            message: message.into(),
            source: Some(source.into()),
        }
    }

    pub fn topology(message: impl Into<String>) -> Self {
        Self::Topology {
            message: message.into(),
        }
    }

    pub fn kernel(message: impl Into<String>) -> Self {
        Self::Kernel {
            message: message.into(),
            source: None,
        }
    }

    pub fn kernel_because(message: impl Into<String>, source: impl Into<BoxError>) -> Self {
        Self::Kernel {
            message: message.into(),
            source: Some(source.into()),
        }
    }

    pub fn rendering(message: impl Into<String>) -> Self {
        Self::Rendering {
            message: message.into(),
            source: None,
        }
    }

    pub fn rendering_because(message: impl Into<String>, source: impl Into<BoxError>) -> Self {
        Self::Rendering {
            message: message.into(),
            source: Some(source.into()),
        }
    }

    pub fn io(context: impl Into<String>, source: impl Into<BoxError>) -> Self {
        Self::Io {
            context: context.into(),
            source: source.into(),
        }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported {
            message: message.into(),
        }
    }
}

impl From<std::io::Error> for CadError {
    fn from(source: std::io::Error) -> Self {
        Self::io("filesystem operation failed", source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_chain_survives_wrapping() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = CadError::io("opening document", io);

        assert_eq!(err.kind(), ErrorKind::Io);
        let source = std::error::Error::source(&err).expect("io variant always carries a source");
        assert!(source.to_string().contains("denied"));
    }

    #[test]
    fn variants_without_a_cause_report_none() {
        let err = CadError::topology("ExtrudeCap(Top) of feature 3 no longer exists");
        assert_eq!(err.kind(), ErrorKind::Topology);
        assert!(std::error::Error::source(&err).is_none());
    }

    #[test]
    fn a_constraint_failure_carries_a_cause_only_when_it_was_given_one() {
        // The plain constructor is what almost every constraint refusal uses,
        // and a caller asking what one was about has to be told nothing rather
        // than something that happens to be lying around.
        let plain = CadError::constraint("this sketch names a point twice");
        assert_eq!(plain.kind(), ErrorKind::Constraint);
        assert!(std::error::Error::source(&plain).is_none());

        let cause = std::io::Error::other("the thing it was about");
        let carried = CadError::constraint_because("this sketch cannot hold", cause);
        assert_eq!(carried.kind(), ErrorKind::Constraint);
        let source =
            std::error::Error::source(&carried).expect("a cause was given and must be reachable");
        assert!(source.to_string().contains("the thing it was about"));

        // And the message is the whole of what is shown. A caller that read
        // the cause out of the sentence would be reading something that was
        // never put there.
        assert!(!carried.to_string().contains("the thing it was about"));
    }

    #[test]
    fn rendering_is_not_misclassified_as_a_geometry_kernel_failure() {
        let err = CadError::rendering("the graphics device was lost");
        assert_eq!(err.kind(), ErrorKind::Rendering);
        assert_eq!(err.kind().as_str(), "rendering");
    }
}
