// SPDX-License-Identifier: MIT
use ferritecad_types::{CadError, CanonicalHasher, Result, StableEntityId, normalize_f64};

use crate::handle::{ShapeHandle, SubShapeHandle};
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
    /// A sector (§27D): a right-handed turn about the axis through this many
    /// degrees, starting at the profile, with a start and an end face.
    Partial(PartialTurn),
}

/// The degrees of a partial turn, as a kernel accepts them: finite and
/// strictly between 0 and 360.
///
/// Only what a kernel must re-check to build one sector. Which angles a
/// document may hold is the caller's policy, narrower than this; an adapter
/// never widens a partial angle into a full turn or wraps it around.
#[derive(Debug, Clone, Copy)]
pub struct PartialTurn(f64);

impl PartialTurn {
    pub fn new(degrees: f64) -> Result<Self> {
        if degrees.is_finite() && degrees > 0.0 && degrees < 360.0 {
            Ok(Self(degrees))
        } else {
            Err(CadError::input(format!(
                "a partial turn needs a finite angle strictly between 0° and 360°, got {degrees}"
            )))
        }
    }

    pub fn degrees(self) -> f64 {
        self.0
    }
}

// Finite and positive by construction, so equality by bits is equality.
impl PartialEq for PartialTurn {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}
impl Eq for PartialTurn {}

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
        match self.turn {
            RevolveTurn::Full => {
                hasher.field("turn").str("full");
            }
            // The angle's own bits, only for a sector, so full-turn keys stay.
            RevolveTurn::Partial(turn) => {
                hasher
                    .field("turn")
                    .str("partial")
                    .bytes(&turn.degrees().to_bits().to_le_bytes());
            }
        }
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

/// What cutting one edge away at equal distances asks of a kernel (§29A).
///
/// The edge is a sub-shape handle of the target, obtained from the target's
/// own names, as for [`FilletRequest`]. One edge and one constant distance,
/// measured from the edge along **each** of the two adjacent faces (so the
/// slanted flat is `distance_mm * sqrt(2)` wide). There is no reference face,
/// no second distance and no angle: nothing in this request can depend on which
/// of the edge's two faces a walk happens to meet first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChamferRequest {
    target: ShapeHandle,
    edge: SubShapeHandle,
    distance_mm: f64,
}

impl ChamferRequest {
    /// Refuses an edge that is not an edge of the target, and a distance that
    /// is not a finite positive number. Whether the distance fits the part is
    /// the caller's policy.
    pub fn new(target: ShapeHandle, edge: SubShapeHandle, distance_mm: f64) -> Result<Self> {
        if edge.kind() != crate::SubShapeKind::Edge {
            return Err(CadError::input(format!(
                "a chamfer cuts an edge, and {edge} is a {}",
                edge.kind()
            )));
        }
        if edge.shape() != target {
            return Err(CadError::input(format!(
                "a chamfer cuts an edge of the shape it modifies, and {edge} belongs to {}",
                edge.shape()
            )));
        }
        if !distance_mm.is_finite() || distance_mm <= 0.0 {
            return Err(CadError::input(format!(
                "a chamfer distance must be finite and positive, found {distance_mm}"
            )));
        }
        Ok(Self {
            target,
            edge,
            distance_mm,
        })
    }

    pub fn target(&self) -> ShapeHandle {
        self.target
    }

    pub fn edge(&self) -> SubShapeHandle {
        self.edge
    }

    pub fn distance_mm(&self) -> f64 {
        self.distance_mm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §28A: only an edge of the target, and only a finite positive radius.
    #[test]
    fn a_fillet_request_names_an_edge_of_its_target_and_a_real_radius() {
        use crate::handle::{SessionId, ShapeHandle, SubShapeHandle, SubShapeKind};
        let target = ShapeHandle::new(SessionId::new(), 0);
        let edge = SubShapeHandle::new(target, SubShapeKind::Edge, 3);
        let request = FilletRequest::new(target, edge, 2.5).expect("valid");
        assert_eq!(
            (request.target(), request.edge(), request.radius_mm()),
            (target, edge, 2.5)
        );
        let foreign =
            SubShapeHandle::new(ShapeHandle::new(SessionId::new(), 0), SubShapeKind::Edge, 3);
        assert!(FilletRequest::new(target, foreign, 2.5).is_err());
        for kind in [SubShapeKind::Face, SubShapeKind::Vertex] {
            assert!(FilletRequest::new(target, SubShapeHandle::new(target, kind, 3), 2.5).is_err());
        }
        for radius in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                FilletRequest::new(target, edge, radius).is_err(),
                "{radius}"
            );
        }
    }

    /// §29A: only an edge of the target, and only a finite positive distance.
    #[test]
    fn a_chamfer_request_names_an_edge_of_its_target_and_a_real_distance() {
        use crate::handle::{SessionId, ShapeHandle, SubShapeHandle, SubShapeKind};
        let target = ShapeHandle::new(SessionId::new(), 0);
        let edge = SubShapeHandle::new(target, SubShapeKind::Edge, 3);
        let request = ChamferRequest::new(target, edge, 2.5).expect("valid");
        assert_eq!(
            (request.target(), request.edge(), request.distance_mm()),
            (target, edge, 2.5)
        );
        let foreign =
            SubShapeHandle::new(ShapeHandle::new(SessionId::new(), 0), SubShapeKind::Edge, 3);
        assert!(ChamferRequest::new(target, foreign, 2.5).is_err());
        for kind in [SubShapeKind::Face, SubShapeKind::Vertex] {
            assert!(
                ChamferRequest::new(target, SubShapeHandle::new(target, kind, 3), 2.5).is_err()
            );
        }
        for distance in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                ChamferRequest::new(target, edge, distance).is_err(),
                "{distance}"
            );
        }
    }

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

/// What rounding one edge asks of a kernel (§28A).
///
/// The edge is a sub-shape handle of the target, obtained from the target's
/// own names — never an index into a traversal and never a point the edge
/// passes near. One edge and one constant radius: chains, variable radii and
/// fillet-all are not what this request can say.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilletRequest {
    target: ShapeHandle,
    edge: SubShapeHandle,
    radius_mm: f64,
}

impl FilletRequest {
    /// Refuses an edge that is not an edge of the target, and a radius that is
    /// not a finite positive number. Whether the radius fits the part is the
    /// caller's policy; this is only what no kernel could mean.
    pub fn new(target: ShapeHandle, edge: SubShapeHandle, radius_mm: f64) -> Result<Self> {
        if edge.kind() != crate::SubShapeKind::Edge {
            return Err(CadError::input(format!(
                "a fillet rounds an edge, and {edge} is a {}",
                edge.kind()
            )));
        }
        if edge.shape() != target {
            return Err(CadError::input(format!(
                "a fillet rounds an edge of the shape it modifies, and {edge} belongs to {}",
                edge.shape()
            )));
        }
        if !radius_mm.is_finite() || radius_mm <= 0.0 {
            return Err(CadError::input(format!(
                "a fillet radius must be finite and positive, found {radius_mm}"
            )));
        }
        Ok(Self {
            target,
            edge,
            radius_mm,
        })
    }

    pub fn target(&self) -> ShapeHandle {
        self.target
    }

    pub fn edge(&self) -> SubShapeHandle {
        self.edge
    }

    pub fn radius_mm(&self) -> f64 {
        self.radius_mm
    }
}
