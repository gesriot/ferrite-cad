// SPDX-License-Identifier: MIT
//! Grid steps shared by the sketch editors.
/// Editor-only step. Not model data, not a preference, not persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Snap {
    #[default]
    Off,
    Tenth,
    One,
    Five,
    Ten,
}

impl Snap {
    pub(crate) const CHOICES: [(Self, &'static str); 5] = [
        (Self::Off, "Off"),
        (Self::Tenth, "0.1 mm"),
        (Self::One, "1 mm"),
        (Self::Five, "5 mm"),
        (Self::Ten, "10 mm"),
    ];
    pub(crate) fn label(self) -> &'static str {
        Snap::CHOICES
            .iter()
            .find(|(value, _)| *value == self)
            .map(|(_, label)| *label)
            .expect("every Snap value has a label")
    }
    pub(crate) fn round(self, value: f64) -> f64 {
        let snapped = match self {
            Self::Off => return value,
            // Multiply/divide by ten instead of by the inexact binary 0.1:
            // decimal halves round away from zero, and 3/10 displays as "0.3".
            Self::Tenth => (value * 10.0).round() / 10.0,
            Self::One => value.round(),
            Self::Five => (value / 5.0).round() * 5.0,
            Self::Ten => (value / 10.0).round() * 10.0,
        };
        if snapped.is_finite() { snapped } else { value }
    }

    pub(super) fn apply(self, value: f64) -> String {
        let rounded = self.round(value);
        if self != Self::Off && rounded == 0.0 {
            "0".into()
        } else {
            rounded.to_string()
        }
    }
}
