// SPDX-License-Identifier: MIT
//! Drawing into a window's surface, and surviving what happens to one.
//!
//! A surface is the one part of a renderer that the operating system can take
//! away. Windows are resized, minimised, dragged between monitors of different
//! scale factors, and lost outright when a device is reset or a compositor
//! restarts. None of that is exceptional; all of it arrives as an error from
//! the same call.
//!
//! So the decisions are written down here as ordinary functions over ordinary
//! values, and tested without a window: what each surface error means, what a
//! size of zero implies, and what happens to a size larger than the device can
//! hold. A window is needed to *have* one of these problems, and not to decide
//! what to do about it.

use ferritecad_types::{CadError, Result};

use crate::renderer::{PreparedSnapshot, Renderer, RendererId};
use ferritecad_viewport::{Camera, Marked, Visibility};

/// Whether a frame reached the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Presented {
    Drawn,
    /// Nothing was drawn, and nothing is wrong.
    Skipped,
}

/// What to do about what a surface said when asked for its next frame.
///
/// Four outcomes, because there are four different things going on. A texture
/// that arrived is drawn into; a surface that has gone stale can be rebuilt
/// from what is already known; a compositor that was busy, or a window nobody
/// can see, will be ready later and the frame is simply skipped; and a
/// validation failure is a mistake in the program that asking again will not
/// correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SurfaceRecovery {
    /// A texture came back. Draw into it.
    Draw,
    /// Rebuild the surface configuration and try once more.
    Reconfigure,
    /// Nothing is wrong that waiting will not fix. Skip this frame.
    Skip,
    /// Not recoverable by drawing differently.
    Fatal,
}

/// What a surface's answer means for this frame.
///
/// A function of the answer alone, so it can be read, tested and argued about
/// without a window in the room. The two variants that carry a texture cannot
/// be built without a device, so the tests cover the five that can – which are
/// exactly the ones with a decision in them.
pub fn recovery_for(outcome: &wgpu::CurrentSurfaceTexture) -> SurfaceRecovery {
    SurfaceOutcome::without_texture(outcome).recovery()
}

/// The part of an acquisition result that does not require a GPU texture.
///
/// Kept separate from [`SurfaceRecovery`] because `Suboptimal` has two
/// consequences: draw this texture *and* reconfigure after presenting it. A
/// single `Draw` value loses the second half of that decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceOutcome<T> {
    Optimal(T),
    Suboptimal(T),
    Lost,
    Outdated,
    Timeout,
    Occluded,
    Validation,
}

impl<T> SurfaceOutcome<T> {
    /// The single-answer policy used by both the public table and the actual
    /// two-answer acquisition state machine.
    fn recovery(&self) -> SurfaceRecovery {
        match self {
            // Suboptimal is drawn, not discarded. The picture is right; it is
            // the configuration that has drifted, and throwing the frame away
            // would show a stutter to fix something the user cannot see.
            Self::Optimal(_) | Self::Suboptimal(_) => SurfaceRecovery::Draw,
            // The surface no longer matches the window. Reconfiguring from the
            // size already held is exactly the fix.
            Self::Lost | Self::Outdated => SurfaceRecovery::Reconfigure,
            // Busy, or not on screen at all. Both end by themselves.
            Self::Timeout | Self::Occluded => SurfaceRecovery::Skip,
            // A validation error is this program's mistake.
            Self::Validation => SurfaceRecovery::Fatal,
        }
    }
}

impl SurfaceOutcome<()> {
    /// Projects out the texture so callers asking only for policy cannot keep
    /// it alive or accidentally present it.
    fn without_texture(outcome: &wgpu::CurrentSurfaceTexture) -> Self {
        match outcome {
            wgpu::CurrentSurfaceTexture::Success(_) => Self::Optimal(()),
            wgpu::CurrentSurfaceTexture::Suboptimal(_) => Self::Suboptimal(()),
            wgpu::CurrentSurfaceTexture::Lost => Self::Lost,
            wgpu::CurrentSurfaceTexture::Outdated => Self::Outdated,
            wgpu::CurrentSurfaceTexture::Timeout => Self::Timeout,
            wgpu::CurrentSurfaceTexture::Occluded => Self::Occluded,
            wgpu::CurrentSurfaceTexture::Validation => Self::Validation,
        }
    }
}

impl SurfaceOutcome<wgpu::SurfaceTexture> {
    fn from_wgpu(outcome: wgpu::CurrentSurfaceTexture) -> Self {
        match outcome {
            wgpu::CurrentSurfaceTexture::Success(texture) => Self::Optimal(texture),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Self::Suboptimal(texture),
            wgpu::CurrentSurfaceTexture::Lost => Self::Lost,
            wgpu::CurrentSurfaceTexture::Outdated => Self::Outdated,
            wgpu::CurrentSurfaceTexture::Timeout => Self::Timeout,
            wgpu::CurrentSurfaceTexture::Occluded => Self::Occluded,
            wgpu::CurrentSurfaceTexture::Validation => Self::Validation,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Acquisition<T> {
    Draw {
        texture: T,
        reconfigure_after: bool,
    },
    Skip,
    Fatal {
        after_reconfigure: bool,
        outcome: SurfaceOutcome<T>,
    },
}

/// Resolves at most two surface answers into one action.
///
/// `retry` owns the reconfiguration as well as the second acquisition. It is
/// called only after `Lost` or `Outdated`, so the production path cannot retry
/// a timeout or validation failure by accident. Keeping both answers in one
/// state machine also makes the `Lost -> Suboptimal` path testable without a
/// window or a real [`wgpu::SurfaceTexture`].
fn acquire_after_one_retry<T>(
    first: SurfaceOutcome<T>,
    retry: impl FnOnce() -> SurfaceOutcome<T>,
) -> Acquisition<T> {
    if first.recovery() == SurfaceRecovery::Reconfigure {
        finish_acquisition(retry(), true)
    } else {
        finish_acquisition(first, false)
    }
}

/// Turns one answer into a terminal action. A recoverable answer is fatal here
/// only when it was the second answer: the one permitted retry is spent.
fn finish_acquisition<T>(outcome: SurfaceOutcome<T>, after_reconfigure: bool) -> Acquisition<T> {
    match outcome {
        SurfaceOutcome::Optimal(texture) => Acquisition::Draw {
            texture,
            reconfigure_after: false,
        },
        SurfaceOutcome::Suboptimal(texture) => Acquisition::Draw {
            texture,
            reconfigure_after: true,
        },
        SurfaceOutcome::Timeout | SurfaceOutcome::Occluded => Acquisition::Skip,
        outcome => Acquisition::Fatal {
            after_reconfigure,
            outcome,
        },
    }
}

/// The size a surface may actually be configured at, or `None` for no size.
///
/// Zero is not an error. A minimised window and a pane dragged shut both report
/// it, and both are ordinary. What must not happen is configuring a surface
/// with it: every backend rejects a zero extent, and the message it produces
/// names neither the window nor the user action that caused it.
///
/// A size beyond what the device can hold is clamped rather than refused. The
/// window is as large as it is; refusing to draw would leave a blank window on
/// a display the device merely cannot address in full.
pub fn usable_size(width: u32, height: u32, limit: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 || limit == 0 {
        return None;
    }
    Some((width.min(limit), height.min(limit)))
}

/// A window's surface, and the configuration it was last given.
#[derive(Debug)]
pub struct WindowSurface {
    surface: wgpu::Surface<'static>,
    /// The device that configured this surface. A surface texture cannot be
    /// drawn by an arbitrary second device even when that device also happens
    /// to support the window.
    renderer: RendererId,
    format: wgpu::TextureFormat,
    alpha_mode: wgpu::CompositeAlphaMode,
    present_mode: wgpu::PresentMode,
    /// The last size that was actually configured, or `None` while the window
    /// has no area. Kept so a reconfigure after a loss does not need the
    /// caller to remember it.
    configured: Option<(u32, u32)>,
    limit: u32,
}

impl WindowSurface {
    /// Takes ownership of a surface and configures it for `renderer`.
    ///
    /// The renderer must have been opened for this surface – see
    /// [`Renderer::for_surface`] – or the adapter is not known to be able to
    /// present to it.
    pub fn new(
        renderer: &Renderer,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        let capabilities = surface.get_capabilities(renderer.adapter());
        let format = *capabilities.formats.first().ok_or_else(|| {
            CadError::rendering(
                "this surface offers no texture format, so there is nothing to draw into it with",
            )
        })?;
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Auto);

        let mut window = Self {
            surface,
            renderer: renderer.id(),
            format,
            alpha_mode,
            // Fifo is the only mode every backend guarantees, and a viewport
            // that tore while a user dragged a model would be blamed on the
            // geometry.
            present_mode: wgpu::PresentMode::Fifo,
            configured: None,
            limit: renderer.max_texture_dimension(),
        };
        window.resize(renderer, width, height)?;
        Ok(window)
    }

    /// The format this surface is drawn into.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// The size last configured, or `None` when the window has no area.
    pub fn configured_size(&self) -> Option<(u32, u32)> {
        self.configured
    }

    /// Whether there is any area to present into.
    pub fn is_drawable(&self) -> bool {
        self.configured.is_some()
    }

    /// Records a new window size and reconfigures if there is one to use.
    ///
    /// A zero size deconfigures rather than failing: the window still exists
    /// and will come back, and the caller should go on running its event loop.
    pub fn resize(&mut self, renderer: &Renderer, width: u32, height: u32) -> Result<()> {
        require_renderer(self.renderer, renderer.id())?;
        self.configured = usable_size(width, height, self.limit);
        self.reconfigure(renderer);
        Ok(())
    }

    /// Applies the current configuration to the surface.
    fn reconfigure(&mut self, renderer: &Renderer) {
        let Some((width, height)) = self.configured else {
            return;
        };
        self.surface.configure(
            renderer.device(),
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: self.format,
                // Whatever the surface already means by its own format. This
                // viewport does not manage a colour pipeline, and choosing a
                // space here would be choosing one on the display's behalf.
                color_space: wgpu::SurfaceColorSpace::Auto,
                width,
                height,
                present_mode: self.present_mode,
                desired_maximum_frame_latency: 2,
                alpha_mode: self.alpha_mode,
                view_formats: Vec::new(),
            },
        );
    }

    /// Takes the frame's one texture, so several things can draw into it.
    ///
    /// A window frame is composed, not drawn once: the model goes in, then the
    /// interface on top of it, and only then is the whole thing published. A
    /// surface hands out one texture per frame, so that sequence has to share
    /// one – an overlay that acquired its own would be asking for a second
    /// frame, and one that ran after `present` would be drawing into something
    /// already on screen.
    ///
    /// Returns `None` when there is nothing to draw into: a window with no
    /// area, one nobody can see, or a frame the compositor was not ready to
    /// hand over. None of those is an error.
    ///
    /// The returned frame borrows this surface for as long as it lives, which
    /// is what stops the window being reconfigured underneath an outstanding
    /// texture. That is not tidiness: wgpu documents `configure` as panicking
    /// while a surface texture is alive, so the borrow turns a crash into a
    /// compile error.
    ///
    /// ```compile_fail
    /// # use ferritecad_viewport_gpu::{Renderer, WindowSurface};
    /// fn resize_while_a_frame_is_open(surface: &mut WindowSurface, renderer: &mut Renderer) {
    ///     let frame = surface.begin(renderer);
    ///     // The surface is borrowed by the frame, so this does not compile.
    ///     let _ = surface.resize(renderer, 800, 600);
    ///     drop(frame);
    /// }
    /// ```
    ///
    /// The same code without the resize does compile, which is what says the
    /// refusal above is about the borrow and not about a typo:
    ///
    /// ```no_run
    /// # use ferritecad_viewport_gpu::{PreparedSnapshot, Renderer, WindowSurface};
    /// # use ferritecad_viewport::{Camera, Marked, Visibility};
    /// fn compose(
    ///     surface: &mut WindowSurface,
    ///     renderer: &mut Renderer,
    ///     prepared: &PreparedSnapshot,
    ///     camera: &Camera,
    ///     visibility: &Visibility,
    /// ) -> ferritecad_types::Result<()> {
    ///     let Some(frame) = surface.begin(renderer)? else {
    ///         return Ok(()); // Nothing to draw into, and nothing wrong.
    ///     };
    ///     let frame =
    ///         frame.draw_scene(prepared, camera, Marked::Nothing, Marked::Nothing, visibility)?;
    ///     // An interface would draw into `frame.view()` here, on top of the
    ///     // model and before anything is published.
    ///     frame.present();
    ///     Ok(())
    /// }
    /// ```
    pub fn begin<'a>(&'a mut self, renderer: &'a mut Renderer) -> Result<Option<SurfaceFrame<'a>>> {
        require_renderer(self.renderer, renderer.id())?;
        if !self.is_drawable() {
            return Ok(None);
        }

        let first = SurfaceOutcome::from_wgpu(self.surface.get_current_texture());
        let acquisition = acquire_after_one_retry(first, || {
            self.reconfigure(renderer);
            SurfaceOutcome::from_wgpu(self.surface.get_current_texture())
        });
        let (texture, reconfigure_after) = match acquisition {
            Acquisition::Draw {
                texture,
                reconfigure_after,
            } => (texture, reconfigure_after),
            Acquisition::Skip => return Ok(None),
            Acquisition::Fatal {
                after_reconfigure,
                outcome,
            } => {
                let message = if after_reconfigure {
                    "this window's surface could not be rebuilt"
                } else {
                    "this window's surface cannot be drawn into"
                };
                return Err(CadError::rendering(format!("{message}: {outcome:?}")));
            }
        };

        let (width, height) = self
            .configured
            .expect("a drawable surface has a configured size");
        let view = texture.texture.create_view(&Default::default());
        Ok(Some(SurfaceFrame {
            surface: self,
            renderer,
            texture: Some(texture),
            view,
            reconfigure_after,
            width,
            height,
        }))
    }

    /// Draws a prepared snapshot into the window and presents it.
    ///
    /// The whole of a frame when there is nothing above the model. Composed
    /// from [`Self::begin`] rather than beside it, so a window with an
    /// interface and one without take the same path through the same
    /// acquisition and the same reconfiguration.
    ///
    /// Returns [`Presented::Skipped`] when there was nothing to draw into – a
    /// window with no area, or a frame the compositor was not ready to hand
    /// over. Neither is an error, and treating them as one would make an
    /// ordinary minimise look like a fault.
    pub fn present(
        &mut self,
        renderer: &mut Renderer,
        prepared: &PreparedSnapshot,
        camera: &Camera,
        selected: Marked,
        hovered: Marked,
        visibility: &Visibility,
    ) -> Result<Presented> {
        let Some(frame) = self.begin(renderer)? else {
            return Ok(Presented::Skipped);
        };
        let frame = frame.draw_scene(prepared, camera, selected, hovered, visibility)?;
        frame.present();
        Ok(Presented::Drawn)
    }
}

/// One frame's texture, before its clearing scene pass.
///
/// Holds both the window's surface and its renderer for as long as it lives:
/// nothing can resize the surface or substitute another device while a texture
/// is outstanding. It intentionally exposes no texture view; only
/// [`Self::draw_scene`] advances it into the state an overlay may use.
///
/// ```compile_fail
/// # use ferritecad_viewport_gpu::SurfaceFrame;
/// fn overlay_before_the_scene(frame: &SurfaceFrame<'_>) {
///     let _ = frame.view();
/// }
/// ```
#[derive(Debug)]
pub struct SurfaceFrame<'a> {
    surface: &'a mut WindowSurface,
    renderer: &'a mut Renderer,
    /// Taken by [`Self::present`]. Dropping it without presenting discards the
    /// frame, which is what a caller that gave up part way through means.
    texture: Option<wgpu::SurfaceTexture>,
    view: wgpu::TextureView,
    reconfigure_after: bool,
    width: u32,
    height: u32,
}

impl<'a> SurfaceFrame<'a> {
    /// Draws the model, clearing first.
    ///
    /// Consumes the uncomposed frame and returns the only type that exposes its
    /// texture view. This makes "scene first and once" a type property rather
    /// than a call-order convention: an overlay cannot see the view before the
    /// clearing pass, and there is no `draw_scene` on the composed result with
    /// which to erase it later.
    /// `selected` is drawn as chosen and `hovered` as merely pointed at, each
    /// covering all of its placements. A pick from another snapshot is neither:
    /// see `Renderer::write_globals`.
    pub fn draw_scene(
        self,
        prepared: &PreparedSnapshot,
        camera: &Camera,
        selected: Marked,
        hovered: Marked,
        visibility: &Visibility,
    ) -> Result<ComposedSurfaceFrame<'a>> {
        self.renderer.draw_into(
            prepared,
            camera,
            selected,
            hovered,
            visibility,
            &self.view,
            self.surface.format,
            self.width,
            self.height,
        )?;
        Ok(ComposedSurfaceFrame { frame: self })
    }
}

/// A surface frame after its clearing scene pass and before publication.
///
/// This is the only state that exposes the target to overlays, so drawing an
/// interface before the scene or clearing it away afterwards cannot be
/// expressed through the public API.
#[derive(Debug)]
pub struct ComposedSurfaceFrame<'a> {
    frame: SurfaceFrame<'a>,
}

impl ComposedSurfaceFrame<'_> {
    /// What to draw an overlay into, after the model and before publication.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.frame.view
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.frame.surface.format
    }

    pub fn size(&self) -> (u32, u32) {
        (self.frame.width, self.frame.height)
    }

    /// The device that owns the target, for an overlay renderer.
    pub fn device(&self) -> &wgpu::Device {
        self.frame.renderer.device()
    }

    /// The queue that presents the target, for an overlay renderer.
    pub fn queue(&self) -> &wgpu::Queue {
        self.frame.renderer.queue()
    }

    /// Publishes the composed frame.
    ///
    /// Consuming, because a surface texture is presented exactly once. The
    /// reconfiguration a suboptimal acquisition asked for happens here, after
    /// the texture has gone: reconfiguring while it was still alive is the
    /// panic the borrow above exists to prevent.
    pub fn present(mut self) {
        if let Some(texture) = self.frame.texture.take() {
            self.frame.renderer.present(texture);
        }
        if self.frame.reconfigure_after {
            self.frame.surface.reconfigure(self.frame.renderer);
        }
    }
}

/// A surface texture belongs to the device that configured its surface.
///
/// Checked before resize changes remembered state and before present acquires a
/// texture. A late driver validation error would name neither renderer and
/// would make the ownership mistake much harder to find than it was to make.
fn require_renderer(expected: RendererId, actual: RendererId) -> Result<()> {
    if expected != actual {
        return Err(CadError::rendering(format!(
            "this window surface belongs to {expected} and cannot be used by {actual}: its \
             textures belong to the other device"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_with_no_area_has_no_size_to_configure() {
        // Minimised windows, panes dragged shut, and the moment before a first
        // layout. Every backend rejects a zero extent, and the message names
        // neither the window nor what the user did.
        assert_eq!(usable_size(0, 600, 8192), None);
        assert_eq!(usable_size(800, 0, 8192), None);
        assert_eq!(usable_size(0, 0, 8192), None);
        assert_eq!(usable_size(800, 600, 8192), Some((800, 600)));
    }

    #[test]
    fn a_window_larger_than_the_device_is_clamped_rather_than_refused() {
        // The window is as large as it is. Refusing to draw would leave it
        // blank on a display the device merely cannot address in full.
        assert_eq!(usable_size(20_000, 600, 8192), Some((8192, 600)));
        assert_eq!(usable_size(800, 20_000, 8192), Some((800, 8192)));
        assert_eq!(usable_size(u32::MAX, u32::MAX, 8192), Some((8192, 8192)));

        // A device that can hold nothing is not a device to clamp against.
        assert_eq!(usable_size(800, 600, 0), None);
    }

    #[test]
    fn every_answer_a_surface_can_give_has_one_response() {
        // Stale: rebuild from what is already known.
        assert_eq!(
            recovery_for(&wgpu::CurrentSurfaceTexture::Lost),
            SurfaceRecovery::Reconfigure
        );
        assert_eq!(
            recovery_for(&wgpu::CurrentSurfaceTexture::Outdated),
            SurfaceRecovery::Reconfigure
        );

        // Busy, or not on screen. Both end by themselves, and redrawing the
        // last frame or blocking for the next both look worse than missing one.
        assert_eq!(
            recovery_for(&wgpu::CurrentSurfaceTexture::Timeout),
            SurfaceRecovery::Skip
        );
        assert_eq!(
            recovery_for(&wgpu::CurrentSurfaceTexture::Occluded),
            SurfaceRecovery::Skip
        );

        // This program's own mistake, which retrying would bury.
        assert_eq!(
            recovery_for(&wgpu::CurrentSurfaceTexture::Validation),
            SurfaceRecovery::Fatal
        );
    }

    #[test]
    fn a_suboptimal_retry_is_drawn_and_then_reconfigured() {
        let retries = std::cell::Cell::new(0);
        let acquired = acquire_after_one_retry(SurfaceOutcome::Lost, || {
            retries.set(retries.get() + 1);
            SurfaceOutcome::Suboptimal(17_u8)
        });

        assert_eq!(retries.get(), 1);
        assert_eq!(
            acquired,
            Acquisition::Draw {
                texture: 17,
                reconfigure_after: true,
            }
        );
    }

    #[test]
    fn a_retry_is_never_a_loop_or_a_retry_of_a_transient_answer() {
        let retries = std::cell::Cell::new(0);
        let second_loss = acquire_after_one_retry(SurfaceOutcome::<()>::Outdated, || {
            retries.set(retries.get() + 1);
            SurfaceOutcome::Lost
        });
        assert_eq!(retries.get(), 1);
        assert_eq!(
            second_loss,
            Acquisition::Fatal {
                after_reconfigure: true,
                outcome: SurfaceOutcome::Lost,
            }
        );

        let timeout = acquire_after_one_retry(SurfaceOutcome::<()>::Timeout, || {
            retries.set(retries.get() + 1);
            SurfaceOutcome::Optimal(())
        });
        assert_eq!(retries.get(), 1, "a timeout must not acquire again");
        assert_eq!(timeout, Acquisition::Skip);
    }

    #[test]
    fn a_window_surface_cannot_move_between_renderers() {
        let owner = RendererId::next();
        let other = RendererId::next();

        require_renderer(owner, owner).expect("its own renderer is accepted");
        let error = require_renderer(owner, other).expect_err("another renderer is refused");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Rendering);
        let message = error.to_string();
        assert!(message.contains(&owner.to_string()), "{message}");
        assert!(message.contains(&other.to_string()), "{message}");
        assert!(!message.contains("  "), "broken whitespace: {message}");
    }
}
