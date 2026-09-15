// SPDX-License-Identifier: MIT
use ferritecad_types::{
    CadError, CanonicalHasher, Point3, ProfileJoint, Result, StableEntityId, Vec3, normalize_f64,
};

/// A point in a sketch plane's own coordinates, in millimetres.
///
/// Named apart from `ferritecad_document::Point2` on purpose: that one is a
/// stored payload field, this one is an argument to a library call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanarPoint {
    pub x: f64,
    pub y: f64,
}

impl PlanarPoint {
    pub const ORIGIN: Self = Self { x: 0.0, y: 0.0 };

    pub fn new(x: f64, y: f64) -> Result<Self> {
        Ok(Self {
            x: normalize_f64(x)?,
            y: normalize_f64(y)?,
        })
    }

    fn feed(&self, hasher: &mut CanonicalHasher) {
        const VALIDATED: &str = "planar points are validated finite on construction";
        hasher.f64(self.x).expect(VALIDATED);
        hasher.f64(self.y).expect(VALIDATED);
    }
}

/// Where a profile's plane sits in model space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SketchPlane {
    origin: Point3,
    /// Unit vector along the plane's local X axis.
    x_axis: Vec3,
    /// Unit vector along the plane's normal; the extrusion direction.
    normal: Vec3,
}

impl SketchPlane {
    /// Builds a plane, normalising the axes and refusing a degenerate frame.
    ///
    /// The axes are made orthogonal here rather than trusted, because a frame
    /// that is a fraction of a degree off square produces a solid that is
    /// subtly wrong everywhere rather than obviously wrong somewhere.
    pub fn new(origin: Point3, x_axis: Vec3, normal: Vec3) -> Result<Self> {
        let normal = normal.normalized()?;
        let x_axis = x_axis.normalized()?;

        // Remove any component of x along the normal, then renormalise.
        let projection = x_axis.dot(normal);
        let orthogonal = Vec3::new(
            x_axis.x - projection * normal.x,
            x_axis.y - projection * normal.y,
            x_axis.z - projection * normal.z,
        )?;
        let x_axis = orthogonal.normalized().map_err(|_| {
            CadError::input("the plane's X axis is parallel to its normal, so it defines no frame")
        })?;

        Ok(Self {
            origin,
            x_axis,
            normal,
        })
    }

    /// The world XY plane, with X along the world X axis.
    pub fn world_xy() -> Self {
        Self {
            origin: Point3::ORIGIN,
            x_axis: Vec3::X,
            normal: Vec3::Z,
        }
    }

    pub fn origin(&self) -> Point3 {
        self.origin
    }

    pub fn x_axis(&self) -> Vec3 {
        self.x_axis
    }

    pub fn normal(&self) -> Vec3 {
        self.normal
    }

    /// The plane's local Y axis, completing a right-handed frame.
    pub fn y_axis(&self) -> Vec3 {
        self.normal
            .cross(self.x_axis)
            .expect("axes are unit and orthogonal by construction, so the cross product is finite")
    }

    /// Maps a point from plane coordinates into model space.
    pub fn to_model(&self, point: PlanarPoint) -> Result<Point3> {
        let y_axis = self.y_axis();
        Point3::new(
            self.origin.x + self.x_axis.x * point.x + y_axis.x * point.y,
            self.origin.y + self.x_axis.y * point.x + y_axis.y * point.y,
            self.origin.z + self.x_axis.z * point.x + y_axis.z * point.y,
        )
    }

    fn feed(&self, hasher: &mut CanonicalHasher) {
        const VALIDATED: &str = "plane components are validated finite on construction";
        hasher.field("plane");
        for value in [
            self.origin.x,
            self.origin.y,
            self.origin.z,
            self.x_axis.x,
            self.x_axis.y,
            self.x_axis.z,
            self.normal.x,
            self.normal.y,
            self.normal.z,
        ] {
            hasher.f64(value).expect(VALIDATED);
        }
    }
}

/// The shape of one profile segment, in plane coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum SegmentGeometry {
    Line {
        start: PlanarPoint,
        end: PlanarPoint,
    },
    /// Counter-clockwise from `start_angle` to `end_angle`, in radians.
    Arc {
        center: PlanarPoint,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    /// A whole circle.
    ///
    /// Unlike the other two this curve closes on itself, so it has no
    /// endpoints and meets nothing at a corner. It is a loop on its own rather
    /// than a link in a chain, which is why [`ProfileLoop::closed_curve`]
    /// exists and why [`SegmentGeometry::start`] refuses it: a point picked
    /// off a circle to stand in for an endpoint would be a vertex nobody drew.
    Circle { center: PlanarPoint, radius: f64 },
}

impl SegmentGeometry {
    pub fn line(start: PlanarPoint, end: PlanarPoint) -> Result<Self> {
        if (start.x - end.x).abs() < f64::EPSILON && (start.y - end.y).abs() < f64::EPSILON {
            return Err(CadError::input(
                "a line segment needs two distinct endpoints",
            ));
        }
        Ok(Self::Line { start, end })
    }

    pub fn circle(center: PlanarPoint, radius: f64) -> Result<Self> {
        let radius = normalize_f64(radius)?;
        if radius <= 0.0 {
            return Err(CadError::input(format!(
                "a circle needs a positive radius, got {radius}"
            )));
        }
        Ok(Self::Circle { center, radius })
    }

    /// Whether this curve closes on itself, and so is a whole loop.
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Circle { .. })
    }

    pub fn arc(center: PlanarPoint, radius: f64, start_angle: f64, end_angle: f64) -> Result<Self> {
        let radius = normalize_f64(radius)?;
        if radius <= 0.0 {
            return Err(CadError::input(format!(
                "an arc needs a positive radius, got {radius}"
            )));
        }
        Ok(Self::Arc {
            center,
            radius,
            start_angle: normalize_f64(start_angle)?,
            end_angle: normalize_f64(end_angle)?,
        })
    }

    pub fn start(&self) -> Result<PlanarPoint> {
        match self {
            Self::Line { start, .. } => Ok(*start),
            Self::Arc {
                center,
                radius,
                start_angle,
                ..
            } => PlanarPoint::new(
                center.x + radius * start_angle.cos(),
                center.y + radius * start_angle.sin(),
            ),
            Self::Circle { .. } => Err(no_endpoints()),
        }
    }

    pub fn end(&self) -> Result<PlanarPoint> {
        match self {
            Self::Line { end, .. } => Ok(*end),
            Self::Arc {
                center,
                radius,
                end_angle,
                ..
            } => PlanarPoint::new(
                center.x + radius * end_angle.cos(),
                center.y + radius * end_angle.sin(),
            ),
            Self::Circle { .. } => Err(no_endpoints()),
        }
    }

    fn feed(&self, hasher: &mut CanonicalHasher) {
        const VALIDATED: &str = "segment components are validated finite on construction";
        match self {
            Self::Line { start, end } => {
                hasher.field("line");
                start.feed(hasher);
                end.feed(hasher);
            }
            Self::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                hasher.field("arc");
                center.feed(hasher);
                hasher.f64(*radius).expect(VALIDATED);
                hasher.f64(*start_angle).expect(VALIDATED);
                hasher.f64(*end_angle).expect(VALIDATED);
            }
            Self::Circle { center, radius } => {
                hasher.field("circle");
                center.feed(hasher);
                hasher.f64(*radius).expect(VALIDATED);
            }
        }
    }
}

/// Said in one place because it is one fact about one kind of curve.
fn no_endpoints() -> CadError {
    CadError::input("a closed curve has no endpoints, so it is a whole loop rather than a segment")
}

/// One segment of a profile, labelled by the caller.
///
/// The label is opaque to the kernel: it is never interpreted, only echoed back
/// in the operation history. That is what lets the topology layer say "the face
/// raised from this segment" without the kernel knowing what a topology
/// reference is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfileSegment {
    pub label: StableEntityId,
    pub geometry: SegmentGeometry,
}

impl ProfileSegment {
    pub fn new(label: StableEntityId, geometry: SegmentGeometry) -> Self {
        Self { label, geometry }
    }
}

/// One closed boundary: either a chain of segments, or a single closed curve.
///
/// The two are different shapes of the same idea, not one shape with a lenient
/// length check. A chain is held together at corners and has exactly as many
/// corners as segments; a closed curve has no corners at all. Keeping them
/// apart is what lets [`ProfileLoop::joints`] answer honestly for both — a
/// single-segment chain would otherwise have to report a corner where a
/// segment meets itself, which is not a place on the drawing.
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Chain(Vec<ProfileSegment>),
    Closed(ProfileSegment),
}

/// A closed boundary of a planar region.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileLoop {
    shape: Shape,
}

impl ProfileLoop {
    /// Builds a loop from one curve that closes on itself.
    ///
    /// The curve must actually be closed. Wrapping an open segment here would
    /// produce a loop with no corners and an unexplained gap, which is the one
    /// failure the chain constructor exists to prevent.
    pub fn closed_curve(segment: ProfileSegment) -> Result<Self> {
        if !segment.geometry.is_closed() {
            return Err(CadError::input(format!(
                "profile segment {} does not close on itself, so it is a link in a chain rather \
                 than a loop",
                segment.label
            )));
        }
        Ok(Self {
            shape: Shape::Closed(segment),
        })
    }

    /// Builds a loop, checking that it closes and that no label repeats.
    ///
    /// Both checks exist because the failure they prevent is silent. An open
    /// loop makes a face the kernel may still build, differently than intended;
    /// a repeated label makes two different faces answer to one name, and the
    /// second one wins at random.
    pub fn new(segments: Vec<ProfileSegment>) -> Result<Self> {
        if segments.len() < 2 {
            return Err(CadError::input(format!(
                "a closed loop needs at least two segments, got {}",
                segments.len()
            )));
        }

        let mut seen = std::collections::BTreeSet::new();
        for segment in &segments {
            // A closed curve has no endpoints to join to a neighbour, so it
            // cannot be one link of a chain. Refused here rather than left to
            // the join check below, which would report a missing endpoint as a
            // gap in the drawing.
            if segment.geometry.is_closed() {
                return Err(CadError::input(format!(
                    "profile segment {} is a closed curve, which is a whole loop rather than one \
                     segment of a chain",
                    segment.label
                )));
            }
            if !seen.insert(segment.label) {
                return Err(CadError::input(format!(
                    "profile segment label {} appears twice; a label must name one segment",
                    segment.label
                )));
            }
        }

        for (index, segment) in segments.iter().enumerate() {
            let next = &segments[(index + 1) % segments.len()];
            let end = segment.geometry.end()?;
            let start = next.geometry.start()?;
            // A millimetre-scale profile; this is a gap a user could not have
            // meant, not a tolerance decision.
            const JOIN_TOLERANCE: f64 = 1.0e-6;
            if (end.x - start.x).abs() > JOIN_TOLERANCE || (end.y - start.y).abs() > JOIN_TOLERANCE
            {
                return Err(CadError::input(format!(
                    "profile loop is open: segment {} ends at ({}, {}) but the next starts at ({}, {})",
                    segment.label, end.x, end.y, start.x, start.y
                )));
            }
        }

        Ok(Self {
            shape: Shape::Chain(segments),
        })
    }

    pub fn segments(&self) -> &[ProfileSegment] {
        match &self.shape {
            Shape::Chain(segments) => segments,
            Shape::Closed(segment) => std::slice::from_ref(segment),
        }
    }

    /// Whether this loop is one curve that closes on itself.
    ///
    /// Asked by consumers that have to say something per corner: a closed
    /// curve has none, and that is a different answer from "the corners are
    /// ambiguous".
    pub fn is_closed_curve(&self) -> bool {
        matches!(self.shape, Shape::Closed(_))
    }

    /// The unordered pair of segment labels meeting at each corner.
    ///
    /// There is one answer per corner, not necessarily one per distinct pair.
    /// A two-segment loop therefore reports the same pair twice. Consumers
    /// naming topology must treat that pair as ambiguous rather than choosing
    /// one of the two corners by position.
    ///
    /// A loop that is one closed curve reports none: it has no corners, and a
    /// joint of a segment with itself would be a name for a place that is not
    /// on the drawing.
    pub fn joints(&self) -> impl ExactSizeIterator<Item = ProfileJoint> + '_ {
        let corners: &[ProfileSegment] = match &self.shape {
            Shape::Chain(segments) => segments,
            Shape::Closed(_) => &[],
        };
        corners.iter().enumerate().map(move |(index, segment)| {
            let before = corners[(index + corners.len() - 1) % corners.len()].label;
            ProfileJoint::new(before, segment.label)
                .expect("profile segment labels are distinct by construction")
        })
    }

    fn feed(&self, hasher: &mut CanonicalHasher) {
        // The two shapes key apart by name, so a chain and a closed curve
        // never collide even if they ever carried the same segments. Chains
        // keep the bytes they always fed.
        match &self.shape {
            Shape::Chain(segments) => {
                hasher.field("loop").u64(segments.len() as u64);
                for segment in segments {
                    hasher.bytes(&segment.label.to_bytes());
                    segment.geometry.feed(hasher);
                }
            }
            Shape::Closed(segment) => {
                hasher.field("closed-loop");
                hasher.bytes(&segment.label.to_bytes());
                segment.geometry.feed(hasher);
            }
        }
    }
}

/// A planar region: one outer loop and any number of holes.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    plane: SketchPlane,
    outer: ProfileLoop,
    inner: Vec<ProfileLoop>,
}

impl Profile {
    /// Builds a profile, refusing a label reused across loops.
    pub fn new(plane: SketchPlane, outer: ProfileLoop, inner: Vec<ProfileLoop>) -> Result<Self> {
        let mut seen = std::collections::BTreeSet::new();
        for entry in std::iter::once(&outer).chain(inner.iter()) {
            for segment in entry.segments() {
                if !seen.insert(segment.label) {
                    return Err(CadError::input(format!(
                        "profile segment label {} appears in more than one loop",
                        segment.label
                    )));
                }
            }
        }

        Ok(Self {
            plane,
            outer,
            inner,
        })
    }

    pub fn plane(&self) -> &SketchPlane {
        &self.plane
    }

    pub fn outer(&self) -> &ProfileLoop {
        &self.outer
    }

    pub fn inner(&self) -> &[ProfileLoop] {
        &self.inner
    }

    /// Every segment of every loop, outer first.
    pub fn segments(&self) -> impl Iterator<Item = &ProfileSegment> {
        self.outer
            .segments()
            .iter()
            .chain(self.inner.iter().flat_map(|l| l.segments().iter()))
    }

    /// Feeds the profile into a cache key.
    pub fn feed(&self, hasher: &mut CanonicalHasher) {
        hasher.field("profile");
        self.plane.feed(hasher);
        self.outer.feed(hasher);
        hasher.field("holes").u64(self.inner.len() as u64);
        for entry in &self.inner {
            entry.feed(hasher);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Result<ProfileLoop> {
        let corners = [
            PlanarPoint::new(0.0, 0.0)?,
            PlanarPoint::new(10.0, 0.0)?,
            PlanarPoint::new(10.0, 10.0)?,
            PlanarPoint::new(0.0, 10.0)?,
        ];
        let mut segments = Vec::new();
        for (index, start) in corners.iter().enumerate() {
            segments.push(ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(*start, corners[(index + 1) % corners.len()])?,
            ));
        }
        ProfileLoop::new(segments)
    }

    #[test]
    fn a_closed_square_is_accepted() {
        assert_eq!(square().expect("closes").segments().len(), 4);
    }

    #[test]
    fn a_square_has_one_joint_for_each_adjacent_pair() {
        let profile = square().expect("closes");
        let labels: Vec<_> = profile
            .segments()
            .iter()
            .map(|segment| segment.label)
            .collect();
        let joints: Vec<_> = profile.joints().collect();

        assert_eq!(joints.len(), labels.len());
        for index in 0..labels.len() {
            assert_eq!(
                joints[index],
                ProfileJoint::new(
                    labels[(index + labels.len() - 1) % labels.len()],
                    labels[index]
                )
                .expect("the square has distinct labels")
            );
        }
        assert_eq!(
            joints
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            labels.len(),
            "each corner of this profile has a different pair"
        );
    }

    #[test]
    fn two_segments_have_two_corners_but_only_one_unordered_pair() {
        let a = PlanarPoint::new(0.0, 0.0).expect("finite");
        let b = PlanarPoint::new(10.0, 0.0).expect("finite");
        let profile = ProfileLoop::new(vec![
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(a, b).expect("line"),
            ),
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(b, a).expect("line"),
            ),
        ])
        .expect("the two segments close");

        let joints: Vec<_> = profile.joints().collect();
        assert_eq!(joints.len(), 2, "the loop has two geometric corners");
        assert_eq!(
            joints[0], joints[1],
            "the unordered name cannot tell them apart"
        );
    }

    #[test]
    fn an_open_loop_is_refused() {
        let a = PlanarPoint::new(0.0, 0.0).expect("finite");
        let b = PlanarPoint::new(10.0, 0.0).expect("finite");
        let c = PlanarPoint::new(10.0, 10.0).expect("finite");

        let err = ProfileLoop::new(vec![
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(a, b).expect("ok"),
            ),
            // Ends at c, but the first segment starts at a.
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(b, c).expect("ok"),
            ),
        ])
        .expect_err("an open loop is not a profile");
        assert!(err.to_string().contains("open"));
    }

    #[test]
    fn a_repeated_label_is_refused() {
        let label = StableEntityId::new();
        let a = PlanarPoint::new(0.0, 0.0).expect("finite");
        let b = PlanarPoint::new(10.0, 0.0).expect("finite");

        let err = ProfileLoop::new(vec![
            ProfileSegment::new(label, SegmentGeometry::line(a, b).expect("ok")),
            ProfileSegment::new(label, SegmentGeometry::line(b, a).expect("ok")),
        ])
        .expect_err("one label must name one segment");
        assert!(err.to_string().contains("twice"));
    }

    #[test]
    fn a_label_reused_across_loops_is_refused() {
        let outer = square().expect("closes");
        let shared = outer.segments()[0];
        let a = PlanarPoint::new(2.0, 2.0).expect("finite");
        let b = PlanarPoint::new(4.0, 2.0).expect("finite");
        let hole = ProfileLoop::new(vec![
            ProfileSegment::new(shared.label, SegmentGeometry::line(a, b).expect("ok")),
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(b, a).expect("ok"),
            ),
        ])
        .expect("closes");

        let err = Profile::new(SketchPlane::world_xy(), outer, vec![hole])
            .expect_err("a label may not span loops");
        assert!(err.to_string().contains("more than one loop"));
    }

    #[test]
    fn non_finite_coordinates_are_refused() {
        assert!(PlanarPoint::new(f64::NAN, 0.0).is_err());
        assert!(PlanarPoint::new(0.0, f64::INFINITY).is_err());
    }

    #[test]
    fn a_zero_length_line_is_refused() {
        let a = PlanarPoint::new(1.0, 1.0).expect("finite");
        assert!(SegmentGeometry::line(a, a).is_err());
    }

    #[test]
    fn a_non_positive_radius_is_refused() {
        let center = PlanarPoint::ORIGIN;
        assert!(SegmentGeometry::arc(center, 0.0, 0.0, 1.0).is_err());
        assert!(SegmentGeometry::arc(center, -1.0, 0.0, 1.0).is_err());
        assert!(SegmentGeometry::arc(center, f64::NAN, 0.0, 1.0).is_err());
    }

    #[test]
    fn a_degenerate_frame_is_refused() {
        let err = SketchPlane::new(Point3::ORIGIN, Vec3::Z, Vec3::Z)
            .expect_err("X parallel to the normal defines no frame");
        assert!(err.to_string().contains("parallel"));
    }

    #[test]
    fn a_skewed_frame_is_squared_up() {
        // X leaning into the normal must come back orthogonal and unit.
        let plane = SketchPlane::new(
            Point3::ORIGIN,
            Vec3::new(1.0, 0.0, 0.5).expect("finite"),
            Vec3::Z,
        )
        .expect("a recoverable frame");

        assert!(plane.x_axis().dot(plane.normal()).abs() < 1e-12);
        assert!((plane.x_axis().length() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn plane_coordinates_map_into_model_space() {
        let plane = SketchPlane::world_xy();
        let point = plane
            .to_model(PlanarPoint::new(3.0, 4.0).expect("finite"))
            .expect("finite");
        assert_eq!(point, Point3::new(3.0, 4.0, 0.0).expect("finite"));
    }

    fn circle(radius: f64) -> Result<ProfileLoop> {
        ProfileLoop::closed_curve(ProfileSegment::new(
            StableEntityId::new(),
            SegmentGeometry::circle(PlanarPoint::new(12.0, -7.0)?, radius)?,
        ))
    }

    #[test]
    fn a_whole_circle_is_one_loop_with_no_corners() {
        let loop_ = circle(10.0).expect("a circle closes on itself");
        assert_eq!(loop_.segments().len(), 1);
        assert!(loop_.is_closed_curve());
        assert_eq!(
            loop_.joints().len(),
            0,
            "a circle has no corner, so it names none"
        );
        assert_eq!(loop_.joints().count(), 0);
        // And it bounds a region like any other loop.
        let profile = Profile::new(SketchPlane::world_xy(), loop_, Vec::new()).expect("valid");
        assert_eq!(profile.segments().count(), 1);
    }

    #[test]
    fn a_circle_has_no_endpoints_to_offer_a_chain() {
        let geometry =
            SegmentGeometry::circle(PlanarPoint::ORIGIN, 4.0).expect("a positive radius");
        assert!(geometry.is_closed());
        for end in [geometry.start(), geometry.end()] {
            assert!(
                end.expect_err("a circle has no endpoints")
                    .to_string()
                    .contains("no endpoints")
            );
        }
        // So it cannot be one link of a chain, in either direction.
        let a = PlanarPoint::new(0.0, 0.0).expect("finite");
        let b = PlanarPoint::new(10.0, 0.0).expect("finite");
        let err = ProfileLoop::new(vec![
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(a, b).expect("ok"),
            ),
            ProfileSegment::new(StableEntityId::new(), geometry),
        ])
        .expect_err("a closed curve is not a segment of a chain");
        assert!(err.to_string().contains("whole loop"));
        // Nor may an open curve be presented as a whole loop.
        let err = ProfileLoop::closed_curve(ProfileSegment::new(
            StableEntityId::new(),
            SegmentGeometry::line(a, b).expect("ok"),
        ))
        .expect_err("a line does not close on itself");
        assert!(err.to_string().contains("does not close"));
    }

    #[test]
    fn a_circle_with_no_radius_is_refused() {
        for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(SegmentGeometry::circle(PlanarPoint::ORIGIN, radius).is_err());
        }
    }

    #[test]
    fn a_circle_keys_by_its_centre_radius_and_kind() {
        let key = |l: ProfileLoop| {
            let mut hasher = CanonicalHasher::new("test");
            Profile::new(SketchPlane::world_xy(), l, Vec::new())
                .expect("valid")
                .feed(&mut hasher);
            hasher.finish()
        };
        let ten = key(circle(10.0).expect("circle"));
        assert_ne!(
            ten,
            key(circle(10.5).expect("circle")),
            "the radius reaches the cache key"
        );
        let elsewhere = ProfileLoop::closed_curve(ProfileSegment::new(
            StableEntityId::new(),
            SegmentGeometry::circle(PlanarPoint::new(0.0, 0.0).expect("finite"), 10.0)
                .expect("circle"),
        ))
        .expect("circle");
        assert_ne!(ten, key(elsewhere), "the centre reaches the cache key");
        // And a chain keys the way it always did, so old cache entries stand.
        let mut hasher = CanonicalHasher::new("test");
        Profile::new(
            SketchPlane::world_xy(),
            square().expect("closes"),
            Vec::new(),
        )
        .expect("valid")
        .feed(&mut hasher);
        let chain = hasher.finish();
        assert_ne!(chain, ten);
    }

    #[test]
    fn geometry_changes_reach_the_cache_key() {
        let plane = SketchPlane::world_xy();
        let one = Profile::new(plane, square().expect("closes"), Vec::new()).expect("valid");

        let mut hasher = CanonicalHasher::new("test");
        one.feed(&mut hasher);
        let first = hasher.finish();

        let mut hasher = CanonicalHasher::new("test");
        one.feed(&mut hasher);
        assert_eq!(hasher.finish(), first, "the same profile keys the same way");

        let other = Profile::new(plane, square().expect("closes"), Vec::new()).expect("valid");
        let mut hasher = CanonicalHasher::new("test");
        other.feed(&mut hasher);
        assert_ne!(
            hasher.finish(),
            first,
            "different segment labels are different inputs"
        );
    }
}
