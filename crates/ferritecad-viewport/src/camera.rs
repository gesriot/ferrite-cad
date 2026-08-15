// SPDX-License-Identifier: MIT
//! Where the view is from, and how big the window is.
//!
//! Separate from the snapshot because the two change for different reasons and
//! at different rates: orbiting the camera does not touch the model, and
//! rebuilding the model does not move the camera. Keeping them apart is what
//! lets a view survive a rebuild.
//!
//! # A window of no size is a normal thing to be handed
//!
//! Minimised windows, collapsed panes and the moment before a first layout all
//! produce a zero width or height. That is not an error and it is not worth an
//! `Option` at every call site, so the arithmetic here stays finite at any size
//! and [`Camera::is_drawable`] says whether there is any point drawing. A
//! projection that divided by a zero aspect ratio would put a `NaN` into a
//! uniform buffer, and the picture would go black one frame later somewhere
//! else entirely.

use ferritecad_types::{CadError, Result};

/// Which way is up, everywhere except a plan view.
///
/// The document's own convention: lengths are millimetres and Z is up. A
/// viewport that chose differently would make every standard view an argument.
const WORLD_UP: [f32; 3] = [0.0, 0.0, 1.0];

/// The directions a drawing names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StandardView {
    /// Looking along +Y, which is the direction a new camera starts in.
    Front,
    Back,
    /// Looking at the left-hand side of the model, from -X.
    Left,
    /// Looking at the right-hand side, from +X.
    Right,
    /// Straight down. North is up, as on a plan.
    Top,
    /// Straight up. North is down, so the view reads as the mirror of the top
    /// rather than as a rotation of it.
    Bottom,
    /// Down the corner between front, right and top.
    Isometric,
}

impl StandardView {
    /// The unit vector from the target towards the eye, and which way is up.
    fn direction_and_up(self) -> ([f32; 3], [f32; 3]) {
        // A third of a right angle either way is what makes an isometric view
        // isometric: the three axes meet the screen at the same angle.
        const ISO: f32 = 0.577_350_3; // 1 / sqrt(3)
        match self {
            Self::Front => ([0.0, -1.0, 0.0], WORLD_UP),
            Self::Back => ([0.0, 1.0, 0.0], WORLD_UP),
            Self::Left => ([-1.0, 0.0, 0.0], WORLD_UP),
            Self::Right => ([1.0, 0.0, 0.0], WORLD_UP),
            Self::Top => ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            Self::Bottom => ([0.0, 0.0, -1.0], [0.0, -1.0, 0.0]),
            Self::Isometric => ([ISO, -ISO, ISO], WORLD_UP),
        }
    }
}

/// Where the near and far planes belong for a view of this size.
///
/// One definition, used by framing and by every interaction, so a camera that
/// has been moved is clipped the same way as one that was just framed.
fn depth_range(distance: f32, radius: f32) -> (f32, f32) {
    let near = (distance - radius).max(radius * 1e-3);
    let far = distance + radius * 1.05;
    (near, far)
}

/// How the world is put on the screen.
///
/// Transient camera state, exactly like where the eye is: not a document fact,
/// not renderer state, not serialised, and no part of what makes one picture
/// the same picture as another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Projection {
    /// What an eye sees: things further away are drawn smaller, and parallel
    /// edges converge. The default, because it is how a model is understood
    /// while it is being built.
    #[default]
    Perspective,
    /// What a drawing shows: equal things are drawn equally wherever they are,
    /// and parallel edges stay parallel. What a plan or an elevation has to be
    /// to be measured off the screen.
    Orthographic,
}

/// Looking at a model from somewhere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    eye: [f32; 3],
    target: [f32; 3],
    up: [f32; 3],
    /// Vertical field of view, radians.
    fov: f32,
    near: f32,
    far: f32,
    /// How the world reaches the screen.
    projection: Projection,
    /// Half the world height the viewport covers at the target plane, used
    /// only while the projection is orthographic.
    ///
    /// Scale rather than distance, because that is what an orthographic view
    /// is: moving the eye along the direction it looks changes nothing about
    /// how big anything is drawn. Switching back to perspective derives a
    /// distance from this, so a zoom made here is respected rather than
    /// discarded in favour of where the eye happened to be beforehand.
    half_height: f32,
    /// How big the thing being looked at is, as [`Camera::frame`] measured it.
    ///
    /// Kept because the clipping range has to follow the distance: a camera
    /// that zoomed in a hundredfold while its near plane stayed where framing
    /// left it would clip away the model it just moved closer to.
    radius: f32,
    width: u32,
    height: u32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            eye: [0.0, -1.0, 0.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            fov: std::f32::consts::FRAC_PI_4,
            near: 0.1,
            far: 1000.0,
            projection: Projection::Perspective,
            half_height: 1.0,
            radius: 1.0,
            width: 0,
            height: 0,
        }
    }
}

impl Camera {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn eye(&self) -> [f32; 3] {
        self.eye
    }

    pub fn target(&self) -> [f32; 3] {
        self.target
    }

    /// Which way is up for this camera, as the matrix uses it.
    pub fn up(&self) -> [f32; 3] {
        let (_, screen_up) = self.screen_axes();
        screen_up
    }

    /// Draws through a different projection, and says whether that changed
    /// anything.
    ///
    /// What is being looked at, the viewing direction, which way is up and the
    /// apparent size are all kept: the same part stays the same size on
    /// screen, and only the way depth is treated changes. Switching into
    /// orthographic also keeps the eye. Asking for the projection that is
    /// already in use changes nothing.
    ///
    /// Going back to perspective derives the distance from the scale that is
    /// on screen now, not from wherever the eye stood before. A zoom made in
    /// an orthographic view is a real change to what is being looked at, and
    /// restoring an obsolete distance would throw it away.
    pub fn set_projection(&mut self, projection: Projection) -> bool {
        if self.projection == projection {
            return false;
        }
        let half_fov = (self.fov * 0.5).tan();
        let mut candidate = *self;
        candidate.projection = projection;
        match projection {
            Projection::Orthographic => {
                // The world height the viewport already covers at the target.
                let half_height = self.distance() * half_fov;
                if !half_height.is_finite() || half_height <= f32::EPSILON {
                    return false;
                }
                candidate.half_height = half_height;
            }
            Projection::Perspective => {
                let distance = self.half_height / half_fov;
                if !distance.is_finite() || distance <= f32::EPSILON {
                    return false;
                }
                let direction = self.direction();
                let eye = [
                    self.target[0] + direction[0] * distance,
                    self.target[1] + direction[1] * distance,
                    self.target[2] + direction[2] * distance,
                ];
                if !usable_eye(eye, self.target) {
                    return false;
                }
                candidate.eye = eye;
            }
        }
        candidate.refresh_depth();
        if candidate
            .view_projection()
            .iter()
            .any(|value| !value.is_finite())
        {
            return false;
        }
        *self = candidate;
        true
    }

    /// Which projection this camera draws through.
    pub fn projection_mode(&self) -> Projection {
        self.projection
    }

    /// Records a new surface size.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    /// Whether the surface has any area to draw into.
    pub fn is_drawable(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Width over height, or 1.0 when there is no area.
    ///
    /// One rather than zero or infinity: it keeps every matrix finite, and a
    /// square projection nothing is drawn through is harmless.
    pub fn aspect(&self) -> f32 {
        if self.width == 0 || self.height == 0 {
            1.0
        } else {
            self.width as f32 / self.height as f32
        }
    }

    /// Points the camera at a box, far enough away to see all of it.
    ///
    /// Refuses a box it cannot make sense of rather than producing a view from
    /// nowhere. An empty model has no bounds to frame and the caller decides
    /// what to show instead.
    pub fn frame(&mut self, bounds: ([f32; 3], [f32; 3])) -> Result<()> {
        let (min, max) = bounds;
        for axis in 0..3 {
            if !min[axis].is_finite() || !max[axis].is_finite() {
                return Err(CadError::input(
                    "a camera cannot frame a model whose extent is not finite",
                ));
            }
            if max[axis] < min[axis] {
                return Err(CadError::input(
                    "a camera cannot frame a box whose maximum is below its minimum",
                ));
            }
        }

        // Halving first avoids overflowing two large, same-sign coordinates
        // merely while finding the point between them.
        let centre = [
            min[0] * 0.5 + max[0] * 0.5,
            min[1] * 0.5 + max[1] * 0.5,
            min[2] * 0.5 + max[2] * 0.5,
        ];
        let extent = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
        if extent.iter().any(|value| !value.is_finite()) {
            return Err(CadError::input(
                "a camera cannot frame a box whose extent exceeds its number format",
            ));
        }
        let radius = extent[0].hypot(extent[1]).hypot(extent[2]) * 0.5;
        if !radius.is_finite() {
            return Err(CadError::input(
                "a camera cannot frame a box whose diagonal exceeds its number format",
            ));
        }
        // A single point still needs a distance to be looked at from, or the
        // eye lands on the target and the view matrix has no direction.
        let radius = if radius > f32::EPSILON { radius } else { 1.0 };

        let vertical_half_fov = self.fov * 0.5;
        let horizontal_half_fov = (vertical_half_fov.tan() * self.aspect()).atan();
        // A sphere fits through a perspective cone at r / sin(theta), not at
        // r / tan(theta). Use the narrower axis, which is horizontal in a
        // portrait viewport, and leave a small margin for f32 rounding.
        let limiting_half_fov = vertical_half_fov.min(horizontal_half_fov);
        let distance = radius / limiting_half_fov.sin() * 1.05;

        // The direction is kept. Framing answers "show me all of it", not
        // "and from the front": a user who has just turned the model to look
        // at a feature and then asks to see the whole thing has not asked to
        // be sent back where they started.
        let direction = self.direction();
        let eye = [
            centre[0] + direction[0] * distance,
            centre[1] + direction[1] * distance,
            centre[2] + direction[2] * distance,
        ];
        if !distance.is_finite() || eye.iter().any(|value| !value.is_finite()) || eye == centre {
            return Err(CadError::input(
                "a camera cannot represent a useful view of a box at this scale",
            ));
        }

        let (near, far) = depth_range(distance, radius);
        if !near.is_finite() || !far.is_finite() || far <= near {
            return Err(CadError::input(
                "a camera cannot represent the clipping range this box requires",
            ));
        }

        // What the perspective fit covers at the target plane is also the
        // orthographic scale. It is slightly more generous than the smallest
        // parallel fit, but keeps framing stable across a later projection
        // change: deriving a perspective distance from this half-height gives
        // back `distance`, so a turned corner cannot leave the view merely
        // because the framed drawing was switched to perspective.
        let half_height = distance * vertical_half_fov.tan();
        if !half_height.is_finite() || half_height <= f32::EPSILON {
            return Err(CadError::input(
                "a camera cannot represent a useful view of a box at this scale",
            ));
        }

        let mut candidate = *self;
        candidate.target = centre;
        candidate.eye = eye;
        candidate.radius = radius;
        candidate.near = near;
        candidate.far = far;
        candidate.half_height = half_height;
        if candidate
            .view_projection()
            .iter()
            .any(|value| !value.is_finite())
        {
            return Err(CadError::input(
                "a camera cannot represent this view without overflowing its matrix",
            ));
        }
        *self = candidate;
        Ok(())
    }

    /// How far the eye is from what it is looking at.
    pub fn distance(&self) -> f32 {
        let offset = sub(self.eye, self.target);
        offset[0].hypot(offset[1]).hypot(offset[2])
    }

    /// The unit vector from the target towards the eye.
    ///
    /// Falls back to looking along +Y – the same direction a new camera starts
    /// in – when the eye has landed on the target and there is no direction to
    /// report.
    fn direction(&self) -> [f32; 3] {
        normalise(sub(self.eye, self.target)).unwrap_or([0.0, -1.0, 0.0])
    }

    /// How much world the target plane covers per pixel of the viewport.
    ///
    /// Zero when there is no viewport to measure against, which makes a drag on
    /// a window of no size move nothing rather than move everything.
    pub fn world_per_pixel(&self) -> f32 {
        if !self.is_drawable() {
            return 0.0;
        }
        let visible_height = match self.projection {
            Projection::Perspective => 2.0 * self.distance() * (self.fov * 0.5).tan(),
            Projection::Orthographic => 2.0 * self.half_height,
        };
        let per_pixel = visible_height / self.height as f32;
        if per_pixel.is_finite() {
            per_pixel
        } else {
            0.0
        }
    }

    /// Swings the eye around the target, keeping its distance.
    ///
    /// `yaw` turns about the world's up axis and `pitch` raises or lowers the
    /// eye, both in radians. The pitch is clamped just short of straight up and
    /// straight down: at exactly vertical the up axis and the view direction
    /// are parallel, there is no side vector to be had, and the view would flip
    /// about an axis the user did not touch.
    ///
    /// Orbiting is defined about the world's up axis, so it restores that as
    /// the camera's up. A view that had been rolled – [`StandardView::Top`]
    /// tilts it so that north is up – is levelled by the first orbit.
    pub fn orbit(&mut self, yaw: f32, pitch: f32) {
        if !yaw.is_finite() || !pitch.is_finite() {
            return;
        }
        let distance = self.distance();
        if !distance.is_finite() || distance <= f32::EPSILON {
            return;
        }

        let offset = sub(self.eye, self.target);
        let current_yaw = offset[1].atan2(offset[0]);
        let current_pitch = (offset[2] / distance).clamp(-1.0, 1.0).asin();

        const LIMIT: f32 = std::f32::consts::FRAC_PI_2 - 1e-3;
        let yaw = current_yaw + yaw;
        let pitch = (current_pitch + pitch).clamp(-LIMIT, LIMIT);

        let horizontal = pitch.cos() * distance;
        let eye = [
            self.target[0] + horizontal * yaw.cos(),
            self.target[1] + horizontal * yaw.sin(),
            self.target[2] + pitch.sin() * distance,
        ];
        self.accept_pose(eye, self.target, WORLD_UP);
    }

    /// Slides the view sideways and up, in pixels of the current viewport.
    ///
    /// Positive `right` moves the view to the right, so what is on screen
    /// appears to move left, by exactly that many pixels at the target plane.
    /// Which way a mouse drag maps onto that is the window layer's business:
    /// this takes camera axes so no windowing system's idea of which way `y`
    /// grows has to be baked in here.
    pub fn pan(&mut self, right: f32, up: f32) {
        if !right.is_finite() || !up.is_finite() {
            return;
        }
        let scale = self.world_per_pixel();
        if scale == 0.0 {
            return;
        }

        let (side, screen_up) = self.screen_axes();

        let shift = [
            (side[0] * right + screen_up[0] * up) * scale,
            (side[1] * right + screen_up[1] * up) * scale,
            (side[2] * right + screen_up[2] * up) * scale,
        ];
        if shift.iter().any(|value| !value.is_finite()) {
            return;
        }

        let mut eye = self.eye;
        let mut target = self.target;
        for (axis, distance) in shift.iter().enumerate() {
            eye[axis] += distance;
            target[axis] += distance;
        }
        self.accept_pose(eye, target, self.up);
    }

    /// Moves the eye towards or away from the target.
    ///
    /// Positive `amount` moves closer. The step is exponential so that a notch
    /// of a wheel covers the same proportion of the remaining distance wherever
    /// it is used, and the distance is clamped: at zero the eye would be inside
    /// what it is looking at and there would be no direction left to look in.
    pub fn zoom(&mut self, amount: f32) {
        // The centre of the viewport is the one point a centred zoom holds
        // still, so this is the anchored zoom asked to hold it.
        self.zoom_at(amount, 0.0, 0.0);
    }

    /// Zooms while keeping one point of the viewport over what is under it.
    ///
    /// `right` and `up` are pixels from the centre of the viewport, positive
    /// right and positive up, the same camera axes [`Camera::pan`] takes. Which
    /// way a windowing system's `y` grows stays outside this crate.
    ///
    /// What is held is the point where the ray through that pixel meets the
    /// plane through the target, square to the viewing direction. That is the
    /// plane every other operation here already measures against, and it needs
    /// nothing read back from a depth buffer. A surface nearer or further than
    /// the target is therefore held approximately rather than exactly, which is
    /// what a wheel over a model feels like anyway; exact anchoring to an
    /// arbitrary surface is a different, more expensive rule.
    ///
    /// The scale changes exactly as a centred zoom would, and the target slides
    /// within the view plane by the amount that puts the anchor back under the
    /// same pixel. In perspective that is a change of distance and a slide; in
    /// an orthographic view the distance and direction are untouched, because
    /// moving the eye along the direction it looks changes nothing a parallel
    /// projection can show.
    pub fn zoom_at(&mut self, amount: f32, right: f32, up: f32) {
        if !amount.is_finite() || !right.is_finite() || !up.is_finite() {
            return;
        }
        let Some(mut candidate) = self.zoomed(amount) else {
            return;
        };
        if candidate == *self {
            // A zero vertical wheel delta, or one too small to change the
            // bounded f32 scale, is no camera operation. In particular, do
            // not rebuild a perspective eye from its normalised direction:
            // that can change one component by an ULP and make a reducer
            // discard questions about a picture that did not move.
            return;
        }

        // How much less world a pixel covers now. The anchor is at the same
        // multiple of the old scale as it must end up of the new one, so the
        // target slides by the pixel offset times the difference between them.
        // Both scales are needed: either alone is a fixed slide that anchors
        // nothing.
        let travel = self.world_per_pixel() - candidate.world_per_pixel();
        if !travel.is_finite() {
            return;
        }
        let (side, screen_up) = self.screen_axes();
        let mut eye = candidate.eye;
        let mut target = candidate.target;
        for axis in 0..3 {
            let shift = (side[axis] * right + screen_up[axis] * up) * travel;
            if !shift.is_finite() {
                return;
            }
            eye[axis] += shift;
            target[axis] += shift;
        }

        if candidate.accept_pose(eye, target, self.up) {
            *self = candidate;
        }
    }

    /// This camera with the zoom applied about its centre, or `None` when the
    /// step cannot be represented.
    ///
    /// One place for what a wheel notch means and how far it may go, so an
    /// anchored zoom cannot drift away from a centred one.
    fn zoomed(&self, amount: f32) -> Option<Self> {
        let mut candidate = *self;
        match self.projection {
            Projection::Orthographic => {
                // Scale, not distance. Moving the eye along the direction it
                // looks changes nothing an orthographic view can show, so a
                // zoom that did that would be a wheel that does nothing.
                let bounded = self.bounded_scale(self.half_height * (-amount).exp())?;
                if bounded == self.half_height {
                    return Some(candidate);
                }
                candidate.half_height = bounded;
            }
            Projection::Perspective => {
                let distance = self.distance();
                if !distance.is_finite() || distance <= f32::EPSILON {
                    return None;
                }
                let bounded = self.bounded_scale(distance * (-amount).exp())?;
                if bounded == distance {
                    return Some(candidate);
                }
                let direction = self.direction();
                candidate.eye = [
                    self.target[0] + direction[0] * bounded,
                    self.target[1] + direction[1] * bounded,
                    self.target[2] + direction[2] * bounded,
                ];
            }
        }
        Some(candidate)
    }

    /// How far in or out a zoom is allowed to reach, measured against the size
    /// of what is being looked at.
    fn bounded_scale(&self, scaled: f32) -> Option<f32> {
        let bounded = scaled.clamp(self.radius * 1e-3, self.radius * 1e5);
        (bounded.is_finite() && bounded > f32::EPSILON).then_some(bounded)
    }

    /// Turns the view around the direction it is already looking in.
    ///
    /// Positive `radians` turns the world counterclockwise on screen, which is
    /// the way the fingers went: what was to the right of the target ends up
    /// above it. Only the camera's idea of which way is up changes, so the
    /// eye, what is being looked at, the distance between them, the
    /// projection, the apparent scale and the clipping range are all exactly
    /// as they were.
    ///
    /// This is not an orbit. Orbiting turns the eye around the model about the
    /// world's up axis and deliberately levels a rolled view; this leaves the
    /// eye where it is and tilts the horizon.
    ///
    /// A rotation too small to move the basis at all does nothing, rather than
    /// replacing the stored up vector with an equal-but-differently-written
    /// one and making a reducer believe the view moved.
    pub fn roll(&mut self, radians: f32) {
        if !radians.is_finite() {
            return;
        }
        let (sin, cos) = radians.sin_cos();
        if !sin.is_finite() || !cos.is_finite() {
            return;
        }

        // The basis the matrix is actually built from, rotated about the
        // direction of view. `side` is `forward x up`, so turning the up
        // vector towards `side` is what carries a point on the right upwards.
        let (side, screen_up) = self.screen_axes();
        let turned = [
            screen_up[0] * cos + side[0] * sin,
            screen_up[1] * cos + side[1] * sin,
            screen_up[2] * cos + side[2] * sin,
        ];
        let (Some(up), Some(current)) = (normalise(turned), normalise(screen_up)) else {
            return;
        };
        if up == current {
            // Nothing the matrix can see has moved. Compared after
            // normalising both, because the basis a view matrix is built from
            // is a cross product and need not already be exactly unit: an
            // angle of nothing must not be an excuse to rewrite the stored up
            // as an equal-but-differently-written vector.
            return;
        }

        let mut candidate = *self;
        candidate.up = up;
        // A basis needs three directions. Were the turned up to land on the
        // viewing direction there would be no sideways left, and the view
        // would flip about an axis nobody asked about.
        if normalise(cross(sub(self.target, self.eye), up)).is_none() {
            return;
        }
        if candidate
            .view_projection()
            .iter()
            .all(|value| value.is_finite())
        {
            *self = candidate;
        }
    }

    /// Looks from one of the directions a drawing would name.
    ///
    /// Keeps the target and the distance: asking for the top view means turning
    /// the model over, not stepping back from it. Top and bottom are looked at
    /// along the world's up axis, where that axis is no use as an up vector, so
    /// they carry their own – north is up, which is what a plan view means.
    pub fn look_from(&mut self, view: StandardView) {
        let distance = self.distance();
        let distance = if distance.is_finite() && distance > f32::EPSILON {
            distance
        } else {
            self.radius.max(f32::EPSILON)
        };

        let (direction, up) = view.direction_and_up();
        let eye = [
            self.target[0] + direction[0] * distance,
            self.target[1] + direction[1] * distance,
            self.target[2] + direction[2] * distance,
        ];
        self.accept_pose(eye, self.target, up);
    }

    /// The camera's own right and up, as unit vectors in world space.
    ///
    /// One definition, so panning and an anchored zoom cannot disagree about
    /// which way the screen lies in the world.
    fn screen_axes(&self) -> ([f32; 3], [f32; 3]) {
        let forward = normalise(sub(self.target, self.eye)).unwrap_or([0.0, 1.0, 0.0]);
        let side = normalise(cross(forward, self.up)).unwrap_or([1.0, 0.0, 0.0]);
        (side, cross(side, forward))
    }

    /// Commits a complete pose, or none of it when the GPU matrix would cease
    /// to be a finite value. Interactive operations return no partial camera.
    ///
    /// Says whether the pose was taken, so an operation that also changes the
    /// scale can discard its whole candidate rather than keep the half of it a
    /// refused pose left behind.
    fn accept_pose(&mut self, eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> bool {
        if !usable_eye(eye, target) {
            return false;
        }
        let mut candidate = *self;
        candidate.eye = eye;
        candidate.target = target;
        candidate.up = up;
        candidate.refresh_depth();
        if candidate
            .view_projection()
            .iter()
            .all(|value| value.is_finite())
        {
            *self = candidate;
            return true;
        }
        false
    }

    /// Puts the clipping range back where the current distance wants it.
    fn refresh_depth(&mut self) {
        let (near, far) = depth_range(self.distance(), self.radius);
        if near.is_finite() && far.is_finite() && far > near {
            self.near = near;
            self.far = far;
        }
    }

    /// The matrix a vertex shader multiplies by, column-major.
    ///
    /// Finite at every size, including none: see the module documentation.
    pub fn view_projection(&self) -> [f32; 16] {
        multiply(&self.projection(), &self.view())
    }

    /// Right-handed look-at, column-major.
    fn view(&self) -> [f32; 16] {
        let forward = normalise(sub(self.target, self.eye)).unwrap_or([0.0, 1.0, 0.0]);
        let side = normalise(cross(forward, self.up)).unwrap_or([1.0, 0.0, 0.0]);
        let up = cross(side, forward);

        [
            side[0],
            up[0],
            -forward[0],
            0.0,
            side[1],
            up[1],
            -forward[1],
            0.0,
            side[2],
            up[2],
            -forward[2],
            0.0,
            -dot(side, self.eye),
            -dot(up, self.eye),
            dot(forward, self.eye),
            1.0,
        ]
    }

    /// Depth in 0..1, which is what wgpu expects, through whichever
    /// projection is in use.
    ///
    /// One place, so the window, the readback, the grid, picking and framing
    /// all see the same view of the world however it is being drawn.
    fn projection(&self) -> [f32; 16] {
        match self.projection {
            Projection::Perspective => self.perspective(),
            Projection::Orthographic => self.orthographic(),
        }
    }

    /// Parallel, with depth in 0..1.
    ///
    /// No division by depth at all: a point's size on screen is its size in
    /// the world, which is what makes an elevation measurable.
    fn orthographic(&self) -> [f32; 16] {
        let depth = self.far - self.near;
        let (scale, offset) = if depth > f32::EPSILON {
            // `view` is right-handed, so what is in front has negative z; the
            // near plane maps to zero and the far plane to one.
            (-1.0 / depth, -self.near / depth)
        } else {
            (-1.0, 0.0)
        };
        let half_height = if self.half_height > f32::EPSILON {
            self.half_height
        } else {
            1.0
        };
        let half_width = half_height * self.aspect();
        let horizontal = if half_width > f32::EPSILON {
            1.0 / half_width
        } else {
            1.0
        };

        [
            horizontal,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0 / half_height,
            0.0,
            0.0,
            0.0,
            0.0,
            scale,
            0.0,
            0.0,
            0.0,
            offset,
            1.0,
        ]
    }

    /// Perspective with depth in 0..1, which is what wgpu expects.
    fn perspective(&self) -> [f32; 16] {
        let focal = 1.0 / (self.fov * 0.5).tan();
        let depth = self.far - self.near;
        // Guarded so a degenerate frustum cannot divide by zero. A camera whose
        // near and far have met shows nothing either way; what matters is that
        // it shows nothing rather than writing a NaN into a uniform.
        let (scale, offset) = if depth > f32::EPSILON {
            // `view` is right-handed: points in front have negative Z. wgpu's
            // depth interval is 0..1, hence clip.w = -view.z and the negative
            // depth scale. The opposite signs put the whole model behind the
            // clip volume even though every matrix entry remains finite.
            // Divide before multiplying: `far * near` can overflow for a
            // large but otherwise representable model even when the final
            // ratio is finite.
            (-self.far / depth, -(self.far / depth) * self.near)
        } else {
            (-1.0, 0.0)
        };

        [
            focal / self.aspect(),
            0.0,
            0.0,
            0.0,
            0.0,
            focal,
            0.0,
            0.0,
            0.0,
            0.0,
            scale,
            -1.0,
            0.0,
            0.0,
            offset,
            0.0,
        ]
    }
}

fn multiply(left: &[f32; 16], right: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for column in 0..4 {
        for row in 0..4 {
            out[column * 4 + row] = (0..4)
                .map(|k| left[k * 4 + row] * right[column * 4 + k])
                .sum();
        }
    }
    out
}

fn sub(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

/// A unit vector, or `None` when there is no direction to normalise.
fn normalise(vector: [f32; 3]) -> Option<[f32; 3]> {
    // Squaring overflows long before a finite `f32` vector does. `hypot`
    // scales its operands, so a large model accepted by `frame()` remains a
    // camera one can orbit and zoom.
    let length = vector[0].hypot(vector[1]).hypot(vector[2]);
    (length > f32::EPSILON && length.is_finite())
        .then(|| [vector[0] / length, vector[1] / length, vector[2] / length])
}

fn usable_eye(eye: [f32; 3], target: [f32; 3]) -> bool {
    eye.iter().all(|value| value.is_finite()) && normalise(sub(eye, target)).is_some()
}
