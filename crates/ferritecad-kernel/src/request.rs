// SPDX-License-Identifier: MIT
use ferritecad_types::{CadError, CanonicalHasher, Result, normalize_f64};

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
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum RevolveAxis {
    PlaneX,
    Line {
        start: crate::PlanarPoint,
        end: crate::PlanarPoint,
    },
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
    /// radians, starting at the profile, with a start and an end face.
    Partial(PartialTurn),
}

/// The radians of a partial turn, as a kernel accepts them: finite and
/// strictly between 0 and 2π.
///
/// Only what a kernel must re-check to build one sector. Which angles a
/// document may hold is the caller's policy, narrower than this; an adapter
/// never widens a partial angle into a full turn or wraps it around.
#[derive(Debug, Clone, Copy)]
pub struct PartialTurn(f64);

impl PartialTurn {
    pub fn new(radians: f64) -> Result<Self> {
        if radians.is_finite() && radians > 0.0 && radians < std::f64::consts::TAU {
            Ok(Self(radians))
        } else {
            Err(CadError::input(format!(
                "a partial turn needs a finite angle strictly between 0 and tau radians, got {radians}"
            )))
        }
    }

    pub fn radians(self) -> f64 {
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
}

impl RevolveRequest {
    /// A profile strictly off the axis: every segment raises a face.
    pub fn new(profile: Profile, axis: RevolveAxis, turn: RevolveTurn) -> Self {
        Self {
            profile,
            axis,
            turn,
        }
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
        hasher.field("axis");
        match self.axis {
            RevolveAxis::PlaneX => {
                hasher.str("plane_x");
            }
            RevolveAxis::PlaneY => {
                hasher.str("plane_y");
            }
            RevolveAxis::Line { start, end } => {
                hasher.str("line");
                start.feed(hasher);
                end.feed(hasher);
            }
        }
        match self.turn {
            RevolveTurn::Full => {
                hasher.field("turn").str("full");
            }
            // The angle's own bits, only for a sector, so full-turn keys stay.
            RevolveTurn::Partial(turn) => {
                hasher
                    .field("turn")
                    .str("partial")
                    .bytes(&turn.radians().to_bits().to_le_bytes());
            }
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

/// Chamfers a nonempty set of target edges at one equal distance along both faces.
/// The builder decides whether that distance is valid for the supplied geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct ChamferRequest {
    target: ShapeHandle,
    edges: Vec<SubShapeHandle>,
    distance_mm: f64,
}

impl ChamferRequest {
    /// Checks edge ownership and a finite positive distance; deduplicates the edge set.
    pub fn new(target: ShapeHandle, edges: &[SubShapeHandle], distance_mm: f64) -> Result<Self> {
        if edges.is_empty() {
            return Err(CadError::input("an edge operation needs at least one edge"));
        }
        for edge in edges {
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
        }
        if !distance_mm.is_finite() || distance_mm <= 0.0 {
            return Err(CadError::input(format!(
                "a chamfer distance must be finite and positive, found {distance_mm}"
            )));
        }
        Ok(Self {
            target,
            edges: edges
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            distance_mm,
        })
    }

    pub fn target(&self) -> ShapeHandle {
        self.target
    }

    pub fn edges(&self) -> &[SubShapeHandle] {
        &self.edges
    }

    pub fn distance_mm(&self) -> f64 {
        self.distance_mm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Target edges are deduplicated; the radius must be finite and positive.
    #[test]
    fn a_fillet_request_names_an_edge_of_its_target_and_a_real_radius() {
        use crate::handle::{SessionId, ShapeHandle, SubShapeHandle, SubShapeKind};
        let target = ShapeHandle::new(SessionId::new(), 0);
        let edge = SubShapeHandle::new(target, SubShapeKind::Edge, 3);
        let request = FilletRequest::new(target, &[edge, edge], 2.5).expect("valid");
        assert_eq!(
            (request.target(), request.edges(), request.radius_mm()),
            (target, [edge].as_slice(), 2.5)
        );
        let foreign =
            SubShapeHandle::new(ShapeHandle::new(SessionId::new(), 0), SubShapeKind::Edge, 3);
        assert!(FilletRequest::new(target, &[edge, foreign], 2.5).is_err());
        assert!(FilletRequest::new(target, &[], 2.5).is_err());
        for kind in [SubShapeKind::Face, SubShapeKind::Vertex] {
            assert!(
                FilletRequest::new(target, &[SubShapeHandle::new(target, kind, 3)], 2.5).is_err()
            );
        }
        for radius in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                FilletRequest::new(target, &[edge], radius).is_err(),
                "{radius}"
            );
        }
    }

    /// Target edges are deduplicated; the distance must be finite and positive.
    #[test]
    fn a_chamfer_request_names_an_edge_of_its_target_and_a_real_distance() {
        use crate::handle::{SessionId, ShapeHandle, SubShapeHandle, SubShapeKind};
        let target = ShapeHandle::new(SessionId::new(), 0);
        let edge = SubShapeHandle::new(target, SubShapeKind::Edge, 3);
        let request = ChamferRequest::new(target, &[edge, edge], 2.5).expect("valid");
        assert_eq!(
            (request.target(), request.edges(), request.distance_mm()),
            (target, [edge].as_slice(), 2.5)
        );
        let foreign =
            SubShapeHandle::new(ShapeHandle::new(SessionId::new(), 0), SubShapeKind::Edge, 3);
        assert!(ChamferRequest::new(target, &[edge, foreign], 2.5).is_err());
        assert!(ChamferRequest::new(target, &[], 2.5).is_err());
        for kind in [SubShapeKind::Face, SubShapeKind::Vertex] {
            assert!(
                ChamferRequest::new(target, &[SubShapeHandle::new(target, kind, 3)], 2.5).is_err()
            );
        }
        for distance in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                ChamferRequest::new(target, &[edge], distance).is_err(),
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

/// The set operation applied to a target and a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanKind {
    Cut,
    Fuse,
    Common,
}

impl BooleanKind {
    pub fn naming_operation(self) -> crate::NamingOperation {
        match self {
            Self::Cut => crate::NamingOperation::Cut,
            Self::Fuse => crate::NamingOperation::Fuse,
            Self::Common => crate::NamingOperation::Common,
        }
    }
}

/// Two inputs of one session; neither is consumed by the operation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BooleanRequest {
    kind: BooleanKind,
    target: ShapeHandle,
    tool: ShapeHandle,
}

impl BooleanRequest {
    pub fn new(kind: BooleanKind, target: ShapeHandle, tool: ShapeHandle) -> Result<Self> {
        if target == tool && kind == BooleanKind::Cut {
            return Err(CadError::input(
                "a cut needs two different shapes; this one names the same shape twice",
            ));
        }
        if target.session() != tool.session() {
            return Err(CadError::input(
                "a boolean needs two shapes of one kernel session",
            ));
        }
        Ok(Self { kind, target, tool })
    }
    pub fn kind(&self) -> BooleanKind {
        self.kind
    }
    pub fn target(&self) -> ShapeHandle {
        self.target
    }
    pub fn tool(&self) -> ShapeHandle {
        self.tool
    }
}

/// Rounds a nonempty set of named target edges at one constant radius.
#[derive(Debug, Clone, PartialEq)]
pub struct FilletRequest {
    target: ShapeHandle,
    edges: Vec<SubShapeHandle>,
    radius_mm: f64,
}

impl FilletRequest {
    /// Checks edge ownership and a finite positive radius; deduplicates the edge set.
    pub fn new(target: ShapeHandle, edges: &[SubShapeHandle], radius_mm: f64) -> Result<Self> {
        if edges.is_empty() {
            return Err(CadError::input("an edge operation needs at least one edge"));
        }
        for edge in edges {
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
        }
        if !radius_mm.is_finite() || radius_mm <= 0.0 {
            return Err(CadError::input(format!(
                "a fillet radius must be finite and positive, found {radius_mm}"
            )));
        }
        Ok(Self {
            target,
            edges: edges
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            radius_mm,
        })
    }

    pub fn target(&self) -> ShapeHandle {
        self.target
    }

    pub fn edges(&self) -> &[SubShapeHandle] {
        &self.edges
    }

    pub fn radius_mm(&self) -> f64 {
        self.radius_mm
    }
}
