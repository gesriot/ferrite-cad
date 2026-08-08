// SPDX-License-Identifier: MIT
use ferritecad_types::{
    CadError, CanonicalHasher, Point3, Result, StableEntityId, Vec3, normalize_f64,
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
        }
    }
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

/// A closed chain of segments.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileLoop {
    segments: Vec<ProfileSegment>,
}

impl ProfileLoop {
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

        Ok(Self { segments })
    }

    pub fn segments(&self) -> &[ProfileSegment] {
        &self.segments
    }

    fn feed(&self, hasher: &mut CanonicalHasher) {
        hasher.field("loop").u64(self.segments.len() as u64);
        for segment in &self.segments {
            hasher.bytes(&segment.label.to_bytes());
            segment.geometry.feed(hasher);
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
