// SPDX-License-Identifier: MIT
use ferritecad_types::{CadError, CanonicalHasher, Result, StableEntityId, normalize_f64};

use crate::handle::ShapeHandle;
use crate::profile::Profile;

/// How far an extrusion runs, and which way.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ExtrudeExtent {
    /// A distance along the profile plane's normal.
    Blind { distance: f64 },
    /// The same distance either side of the profile plane.
    Symmetric { half_distance: f64 },
}

impl ExtrudeExtent {
    pub fn blind(distance: f64) -> Result<Self> {
        Ok(Self::Blind {
            distance: positive(distance, "extrusion distance")?,
        })
    }

    pub fn symmetric(half_distance: f64) -> Result<Self> {
        Ok(Self::Symmetric {
            half_distance: positive(half_distance, "symmetric extrusion half-distance")?,
        })
    }

    /// The total swept length.
    pub fn total_length(self) -> f64 {
        match self {
            Self::Blind { distance } => distance,
            Self::Symmetric { half_distance } => half_distance * 2.0,
        }
    }

    fn feed(&self, hasher: &mut CanonicalHasher) {
        const VALIDATED: &str = "extents are validated finite and positive on construction";
        hasher.field("extent");
        match self {
            Self::Blind { distance } => {
                hasher.str("blind");
                hasher.f64(*distance).expect(VALIDATED);
            }
            Self::Symmetric { half_distance } => {
                hasher.str("symmetric");
                hasher.f64(*half_distance).expect(VALIDATED);
            }
        }
    }
}

/// Sweep a planar profile along its plane's normal.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudeRequest {
    profile: Profile,
    extent: ExtrudeExtent,
    reversed: bool,
}

impl ExtrudeRequest {
    pub fn new(profile: Profile, extent: ExtrudeExtent, reversed: bool) -> Self {
        Self {
            profile,
            extent,
            reversed,
        }
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    pub fn extent(&self) -> ExtrudeExtent {
        self.extent
    }

    /// Runs against the plane normal when true.
    pub fn reversed(&self) -> bool {
        self.reversed
    }

    /// Feeds the request into a cache key.
    ///
    /// The caller must also feed the kernel identity and the tolerance; this
    /// covers only what the request itself contributes.
    pub fn feed(&self, hasher: &mut CanonicalHasher) {
        hasher.field("extrude");
        self.profile.feed(hasher);
        self.extent.feed(hasher);
        hasher.field("reversed").bool(self.reversed);
    }
}

/// The axis a revolution turns about, named in the profile plane's own terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RevolveAxis {
    /// The plane's local Y axis through its origin. In the profile, X is then
    /// the distance from the axis and Y the position along it.
    PlaneY,
}

/// How far a revolution turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RevolveTurn {
    /// Exactly one full turn, 2π. A full turn closes on itself: it has no
    /// start or end face.
    Full,
}

/// Turn a planar profile about an axis in its plane.
///
/// The axis and the angle are stated here, by name, so no adapter supplies
/// them as constants of its own. What profiles are acceptable is the caller's
/// policy; an adapter re-checks what it must to build a valid solid and refuses
/// anything else rather than repairing it.
#[derive(Debug, Clone, PartialEq)]
pub struct RevolveRequest {
    profile: Profile,
    axis: RevolveAxis,
    turn: RevolveTurn,
    axis_segment: Option<StableEntityId>,
}

impl RevolveRequest {
    /// A profile strictly off the axis: every segment raises a face.
    pub fn new(profile: Profile, axis: RevolveAxis, turn: RevolveTurn) -> Self {
        Self {
            profile,
            axis,
            turn,
            axis_segment: None,
        }
    }

    /// §27C: the profile closes on the axis along this segment, which lies on
    /// it and therefore raises no face; every other segment still must.
    ///
    /// The caller's policy decided which segment that is. An adapter checks it
    /// — the segment's ends on the axis, every other vertex off it, no face
    /// from it — and never infers it.
    pub fn with_axis_segment(mut self, label: StableEntityId) -> Result<Self> {
        if !self
            .profile
            .outer()
            .segments()
            .iter()
            .any(|segment| segment.label == label)
        {
            return Err(CadError::input(format!(
                "axis segment {label} is not a segment of the profile being turned"
            )));
        }
        self.axis_segment = Some(label);
        Ok(self)
    }

    /// The segment on the axis, for a profile closed on it.
    pub fn axis_segment(&self) -> Option<StableEntityId> {
        self.axis_segment
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    pub fn axis(&self) -> RevolveAxis {
        self.axis
    }

    pub fn turn(&self) -> RevolveTurn {
        self.turn
    }

    /// Feeds the request into a cache key: the profile (plane, labels and
    /// geometry), the axis and the angle. The caller adds the kernel identity
    /// and the tolerance.
    pub fn feed(&self, hasher: &mut CanonicalHasher) {
        hasher.field("revolve");
        self.profile.feed(hasher);
        hasher.field("axis").str(match self.axis {
            RevolveAxis::PlaneY => "plane_y",
        });
        hasher.field("turn").str(match self.turn {
            RevolveTurn::Full => "full",
        });
        // Only when present, so a profile with a bore keeps the key it had.
        if let Some(label) = self.axis_segment {
            hasher.field("axis_segment").bytes(&label.to_bytes());
        }
    }
}

/// How finely to approximate curved geometry with triangles.
///
/// Part of every mesh cache key. Two tessellations of one solid at different
/// deflections are different results, and serving one under the other's key
/// would put visibly wrong geometry on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TessellationParams {
    linear_deflection: f64,
    angular_deflection: f64,
    relative: bool,
}

impl TessellationParams {
    /// Millimetres of chord error, and radians of angular error.
    ///
    /// The defaults match what the OCCT smoke test uses, so a mesh produced by
    /// the pin workflow and one produced by the application are comparable.
    pub const DEFAULT_LINEAR: f64 = 0.01;
    pub const DEFAULT_ANGULAR: f64 = 0.5;

    pub fn new(linear_deflection: f64, angular_deflection: f64, relative: bool) -> Result<Self> {
        Ok(Self {
            linear_deflection: positive(linear_deflection, "linear deflection")?,
            angular_deflection: positive(angular_deflection, "angular deflection")?,
            relative,
        })
    }

    pub fn linear_deflection(self) -> f64 {
        self.linear_deflection
    }

    pub fn angular_deflection(self) -> f64 {
        self.angular_deflection
    }

    /// Whether the linear deflection scales with the shape's size.
    pub fn relative(self) -> bool {
        self.relative
    }

    /// Feeds the parameters into a cache key.
    pub fn feed(&self, hasher: &mut CanonicalHasher) {
        const VALIDATED: &str = "tessellation parameters are validated on construction";
        hasher.field("tessellation");
        hasher.f64(self.linear_deflection).expect(VALIDATED);
        hasher.f64(self.angular_deflection).expect(VALIDATED);
        hasher.bool(self.relative);
    }
}

impl Default for TessellationParams {
    fn default() -> Self {
        Self {
            linear_deflection: Self::DEFAULT_LINEAR,
            angular_deflection: Self::DEFAULT_ANGULAR,
            relative: false,
        }
    }
}

fn positive(value: f64, what: &str) -> Result<f64> {
    let value = normalize_f64(value)?;
    if value <= 0.0 {
        return Err(CadError::input(format!(
            "{what} must be positive, got {value}"
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_positive_extent_is_refused() {
        assert!(ExtrudeExtent::blind(0.0).is_err());
        assert!(ExtrudeExtent::blind(-5.0).is_err());
        assert!(ExtrudeExtent::symmetric(0.0).is_err());
    }

    #[test]
    fn a_non_finite_extent_is_refused() {
        assert!(ExtrudeExtent::blind(f64::NAN).is_err());
        assert!(ExtrudeExtent::blind(f64::INFINITY).is_err());
    }

    #[test]
    fn a_symmetric_extent_sweeps_twice_its_half_distance() {
        let extent = ExtrudeExtent::symmetric(4.0).expect("positive");
        assert_eq!(extent.total_length(), 8.0);
    }

    #[test]
    fn non_positive_tessellation_parameters_are_refused() {
        assert!(TessellationParams::new(0.0, 0.5, false).is_err());
        assert!(TessellationParams::new(0.01, -0.5, false).is_err());
        assert!(TessellationParams::new(f64::NAN, 0.5, false).is_err());
    }

    #[test]
    fn tessellation_parameters_reach_the_cache_key() {
        let fine = TessellationParams::default();
        let coarse = TessellationParams::new(0.5, 0.5, false).expect("positive");

        let mut hasher = CanonicalHasher::new("test");
        fine.feed(&mut hasher);
        let fine_key = hasher.finish();

        let mut hasher = CanonicalHasher::new("test");
        coarse.feed(&mut hasher);
        assert_ne!(hasher.finish(), fine_key);
    }

    #[test]
    fn the_relative_flag_reaches_the_cache_key() {
        let absolute = TessellationParams::new(0.01, 0.5, false).expect("positive");
        let relative = TessellationParams::new(0.01, 0.5, true).expect("positive");

        let mut hasher = CanonicalHasher::new("test");
        absolute.feed(&mut hasher);
        let absolute_key = hasher.finish();

        let mut hasher = CanonicalHasher::new("test");
        relative.feed(&mut hasher);
        assert_ne!(hasher.finish(), absolute_key);
    }
}

/// Remove the material of one shape from another.
///
/// Two shapes and nothing else. Which faces come back, and what they are
/// called, is not part of the request: naming is what [`CutResult`] reports
/// from the kernel's own history, and a request that could nominate names
/// would be a caller deciding them in advance.
///
/// [`CutResult`]: crate::CutResult
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutRequest {
    target: ShapeHandle,
    tool: ShapeHandle,
}

impl CutRequest {
    /// Refuses the one pair no kernel can mean: a shape cut by itself, which
    /// is empty by definition and would be reported as a boolean that removed
    /// everything rather than as a request nobody could have intended.
    pub fn new(target: ShapeHandle, tool: ShapeHandle) -> Result<Self> {
        if target == tool {
            return Err(CadError::input(
                "a cut needs two different shapes; this one names the same shape twice",
            ));
        }
        if target.session() != tool.session() {
            return Err(CadError::input(
                "a cut needs two shapes of one kernel session",
            ));
        }
        Ok(Self { target, tool })
    }

    /// The shape material is removed from.
    pub fn target(&self) -> ShapeHandle {
        self.target
    }

    /// The shape whose material is removed.
    pub fn tool(&self) -> ShapeHandle {
        self.tool
    }
}
