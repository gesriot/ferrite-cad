// SPDX-License-Identifier: MIT
//! Typed reasons a kernel may refuse to tessellate a shape.

use ferritecad_types::CadError;

/// A tessellation refusal whose meaning callers may act on.
///
/// This vocabulary belongs to the kernel-neutral contract. An adapter may
/// attach one to a [`CadError::Kernel`] while keeping its human-readable
/// message free to change. Callers classify the source through [`Self::of`]
/// and may still show the error's ordinary display text to a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TessellationRefusal {
    /// At least one face has no complete, usable triangle representation.
    IncompleteFace,
}

impl TessellationRefusal {
    /// A stable name for this refusal, for a file or a report that has to
    /// record which one it was.
    ///
    /// Not the display message, which is written for a person and free to
    /// change, and not the `Debug` rendering, which is a debugging aid rather
    /// than a data format. Written out here so a new variant that forgets to
    /// name itself is a compile error rather than a file that records the
    /// wrong reason.
    pub fn stable_name(&self) -> &'static str {
        match self {
            Self::IncompleteFace => "IncompleteFace",
        }
    }

    /// The typed tessellation refusal behind `error`, if it is one.
    ///
    /// Only a direct source of a kernel failure counts. Wrapping this value in
    /// an input, rendering, I/O or other failure does not change that outer
    /// failure's meaning.
    pub fn of(error: &CadError) -> Option<&Self> {
        if !matches!(error, CadError::Kernel { .. }) {
            return None;
        }
        std::error::Error::source(error)?.downcast_ref::<Self>()
    }
}

impl std::fmt::Display for TessellationRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IncompleteFace => f.write_str("one or more faces have no usable triangles"),
        }
    }
}

impl std::error::Error for TessellationRefusal {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_has_a_name_that_is_not_its_message() {
        let refusal = TessellationRefusal::IncompleteFace;
        assert_eq!(refusal.stable_name(), "IncompleteFace");
        assert_ne!(refusal.stable_name(), refusal.to_string());
    }

    #[test]
    fn only_a_typed_direct_kernel_source_is_a_tessellation_refusal() {
        let refusal = CadError::kernel_because(
            "the words shown to a person may change",
            TessellationRefusal::IncompleteFace,
        );
        assert_eq!(
            TessellationRefusal::of(&refusal),
            Some(&TessellationRefusal::IncompleteFace)
        );

        let old_phrase = CadError::kernel("Open CASCADE could not tessellate every face; status 6");
        assert_eq!(TessellationRefusal::of(&old_phrase), None);

        let wrong_outer =
            CadError::input_because("reading failed", TessellationRefusal::IncompleteFace);
        assert_eq!(TessellationRefusal::of(&wrong_outer), None);
    }

    #[test]
    fn changing_only_the_human_message_does_not_change_the_typed_answer() {
        for message in [
            "Open CASCADE could not tessellate every face; status 6",
            "new wording with no implementation name or status text",
        ] {
            let refusal = CadError::kernel_because(message, TessellationRefusal::IncompleteFace);
            assert_eq!(
                TessellationRefusal::of(&refusal),
                Some(&TessellationRefusal::IncompleteFace)
            );
        }
    }
}
