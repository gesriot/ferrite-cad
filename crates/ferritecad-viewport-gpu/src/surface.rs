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

use crate::renderer::{PreparedSnapshot, Renderer};
use ferritecad_viewport::Camera;

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
    match outcome {
        // Suboptimal is drawn, not discarded. The picture is right; it is the
        // configuration that has drifted, and throwing the frame away would
        // show the user a stutter to fix something they cannot see.
        wgpu::CurrentSurfaceTexture::Success(_) | wgpu::CurrentSurfaceTexture::Suboptimal(_) => {
            SurfaceRecovery::Draw
        }
        // The surface no longer matches the window: resized, moved to a display
        // with another format, or the device was reset underneath it.
        // Reconfiguring from the size already held is exactly the fix.
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            SurfaceRecovery::Reconfigure
        }
        // Busy, or not on screen at all. Both end by themselves, and both
        // deserve a skipped frame rather than a spin or an error.
        wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
            SurfaceRecovery::Skip
        }
        // A validation error is this program's mistake. Retrying would hide it
        // behind a stream of identical frames.
        wgpu::CurrentSurfaceTexture::Validation => SurfaceRecovery::Fatal,
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
            format,
            alpha_mode,
            // Fifo is the only mode every backend guarantees, and a viewport
            // that tore while a user dragged a model would be blamed on the
            // geometry.
            present_mode: wgpu::PresentMode::Fifo,
            configured: None,
            limit: renderer.max_texture_dimension(),
        };
        window.resize(renderer, width, height);
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
    pub fn resize(&mut self, renderer: &Renderer, width: u32, height: u32) {
        self.configured = usable_size(width, height, self.limit);
        self.reconfigure(renderer);
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

    /// Draws a prepared snapshot into the window and presents it.
    ///
    /// Returns `Ok(None)` when there was nothing to draw into – a window with
    /// no area, or a frame the compositor was not ready to hand over. Neither
    /// is an error, and treating them as one would make an ordinary minimise
    /// look like a fault.
    ///
    /// A surface that has gone stale is reconfigured and the frame retried
    /// once. Once, and not in a loop: a surface that is stale again immediately
    /// is not going to be fixed by asking a third time, and a renderer that
    /// spun here would hang the event loop that called it.
    pub fn present(
        &mut self,
        renderer: &mut Renderer,
        prepared: &PreparedSnapshot,
        camera: &Camera,
    ) -> Result<Presented> {
        if !self.is_drawable() {
            return Ok(Presented::Skipped);
        }

        let mut suboptimal = false;
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                // Drawn now, reconfigured after presenting: the picture is
                // right and only the configuration has drifted.
                suboptimal = true;
                texture
            }
            other => match recovery_for(&other) {
                SurfaceRecovery::Reconfigure => {
                    self.reconfigure(renderer);
                    // Once, and not in a loop. A surface that is stale again
                    // immediately will not be fixed by a third attempt, and a
                    // renderer that spun here would hang the event loop that
                    // called it.
                    match self.surface.get_current_texture() {
                        wgpu::CurrentSurfaceTexture::Success(texture)
                        | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
                        again => {
                            return match recovery_for(&again) {
                                SurfaceRecovery::Skip => Ok(Presented::Skipped),
                                _ => Err(CadError::rendering(format!(
                                    "this window's surface could not be rebuilt: {again:?}"
                                ))),
                            };
                        }
                    }
                }
                SurfaceRecovery::Skip => return Ok(Presented::Skipped),
                _ => {
                    return Err(CadError::rendering(format!(
                        "this window's surface cannot be drawn into: {other:?}"
                    )));
                }
            },
        };

        let (width, height) = self
            .configured
            .expect("a drawable surface has a configured size");
        let view = texture.texture.create_view(&Default::default());
        renderer.draw_into(prepared, camera, &view, self.format, width, height)?;
        renderer.present(texture);

        if suboptimal {
            self.reconfigure(renderer);
        }
        Ok(Presented::Drawn)
    }
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
}
