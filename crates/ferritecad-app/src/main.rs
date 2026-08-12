// SPDX-License-Identifier: MIT
//! A window showing a model, and nothing else yet.
//!
//! Everything this binary does was decided somewhere it could be tested. What
//! a drag means is `ferritecad-ui`'s and is settled without an event loop; what
//! a frame is, and in what order it is composed, is `ferritecad-viewport-gpu`'s
//! and is settled without a window. What is left here is the wiring, and the
//! aim is that reading it should be enough to see that it is only wiring.
//!
//! # One frame per reason to draw one
//!
//! The event loop waits. It does not redraw on a timer, and it does not redraw
//! once per event: a drag delivers a pointer position for every sample the
//! hardware took, and drawing each one would render the same model at every
//! intermediate place the cursor passed through. Instead every reason to
//! redraw sets a flag, the flag is cleared when a frame is drawn, and
//! `RedrawRequested` is asked for once per batch of reasons.
//!
//! # The scene is fixed, on purpose
//!
//! There is no document here and no geometry kernel. This slice exists to show
//! that a window opens, a surface survives being resized, and the composition
//! order holds; a model arriving from a `.fcad` file is the next thing, and
//! putting it in now would mean debugging two new things at once.

use std::sync::Arc;

use ferritecad_kernel::{
    Mesh, MeshFaceRange, SessionId, ShapeHandle, SubShapeHandle, SubShapeKind,
};
use ferritecad_types::{Result, Transform, Vec3};
use ferritecad_ui::{PointerButton, ViewportEvent, ViewportInput};
use ferritecad_viewport::{RenderSnapshot, SnapshotBuilder, StandardView};
use ferritecad_viewport_gpu::{PreparedSnapshot, Renderer, WindowSurface};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

fn main() -> Result<()> {
    let event_loop = EventLoop::new().map_err(|error| {
        ferritecad_types::CadError::rendering_because("opening a window", error)
    })?;
    // Wait rather than poll: a viewport with nothing happening in it should
    // cost nothing. Every path that changes what is on screen asks for a
    // redraw explicitly.
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App::default();
    event_loop
        .run_app(&mut app)
        .map_err(|error| ferritecad_types::CadError::rendering_because("running the window", error))
}

/// What exists only once a window does.
struct Live {
    window: Arc<Window>,
    renderer: Renderer,
    surface: WindowSurface,
    prepared: PreparedSnapshot,
    egui: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

#[derive(Default)]
struct App {
    live: Option<Live>,
    input: ViewportInput,
}

impl ApplicationHandler for App {
    /// Everything that needs a display is built here and not before.
    ///
    /// `resumed` is the only place a window may be created, and on some
    /// platforms it is called again after the application returns from the
    /// background. Building a second window on the second call is the usual
    /// way that goes wrong, so this returns early when one already exists.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.live.is_some() {
            return;
        }
        match self.start(event_loop) {
            Ok(live) => self.live = Some(live),
            Err(error) => {
                eprintln!("ferritecad: {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(live) = self.live.as_mut() else {
            return;
        };

        // The interface gets first refusal on every event, and says whether it
        // wanted it. What that answer means is the reducer's business.
        let response = live.egui_state.on_window_event(&live.window, &event);
        if response.repaint {
            self.input.request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                // One size, applied to both. The reducer holds the camera and
                // hands back what the surface must be configured with, so the
                // two cannot be given different numbers.
                let (width, height) = self.input.resize(size.width, size.height);
                if let Err(error) = live.surface.resize(&live.renderer, width, height) {
                    eprintln!("ferritecad: {error}");
                    event_loop.exit();
                    return;
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = live.draw(&self.input) {
                    eprintln!("ferritecad: {error}");
                    event_loop.exit();
                }
                // Cleared here and only here: whatever asked for this frame has
                // been paid, and several requests since the last one collapse
                // into this single draw.
                self.input.take_redraw();
                return;
            }
            other => {
                for event in translate(&other) {
                    let claimed = match event {
                        ViewportEvent::Wheel { .. } => {
                            response.consumed || live.egui.egui_wants_pointer_input()
                        }
                        ViewportEvent::PointerPressed(_) => {
                            response.consumed || live.egui.egui_wants_pointer_input()
                        }
                        // A move is claimed only while no gesture is running;
                        // the reducer keeps a drag that began in the viewport.
                        _ => response.consumed,
                    };
                    self.input.handle(event, claimed);
                }
            }
        }

        // One request per batch of reasons. winit coalesces the requests
        // themselves, and the flag makes sure a frame is asked for only when
        // something actually changed.
        if self.input.take_redraw() {
            self.input.request_redraw();
            live.window.request_redraw();
        }
    }
}

impl App {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<Live> {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("FerriteCAD")
                        .with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0)),
                )
                .map_err(|error| {
                    ferritecad_types::CadError::rendering_because("creating the window", error)
                })?,
        );

        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance
            .create_surface(Arc::clone(&window))
            .map_err(|error| {
                ferritecad_types::CadError::rendering_because(
                    "creating a surface for the window",
                    error,
                )
            })?;

        // The adapter is chosen for this surface, not for the machine: see
        // Renderer::for_surface.
        let mut renderer = Renderer::for_surface(&instance, &surface)?;
        let size = window.inner_size();
        let (width, height) = self.input.resize(size.width, size.height);
        let window_surface = WindowSurface::new(&renderer, surface, width, height)?;

        let snapshot = Arc::new(fixed_scene()?);
        if let Some(bounds) = snapshot.bounds() {
            self.input.frame(bounds)?;
        }
        let prepared = renderer.prepare(snapshot)?;

        let egui = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui.clone(),
            egui.viewport_id(),
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let egui_renderer = egui_wgpu::Renderer::new(
            renderer.device(),
            window_surface.format(),
            egui_wgpu::RendererOptions {
                // The interface draws over a frame whose depth buffer belongs
                // to the model's pass and is already finished with.
                depth_stencil_format: None,
                msaa_samples: 1,
                ..Default::default()
            },
        );

        Ok(Live {
            window,
            renderer,
            surface: window_surface,
            prepared,
            egui,
            egui_state,
            egui_renderer,
        })
    }
}

impl Live {
    /// One frame: the model, then the interface, then publication.
    ///
    /// One texture, acquired once. The order is not a convention here – the
    /// seam enforces it, because the model's pass is what clears the target
    /// and the type only offers a view to draw into afterwards.
    fn draw(&mut self, input: &ViewportInput) -> Result<()> {
        let Some(frame) = self.surface.begin(&mut self.renderer)? else {
            // No area, nobody watching, or the compositor was busy. None of
            // those is an error.
            return Ok(());
        };
        let frame = frame.draw_scene(&self.prepared, input.camera())?;

        let raw_input = self.egui_state.take_egui_input(&self.window);
        let output = self.egui.run_ui(raw_input, |ui| {
            // A plain rectangle rather than a panel of controls. This build
            // carries no font: the ones egui bundles are licensed under terms
            // outside this project's allow list, and that is a decision to take
            // on its own rather than to smuggle in with a window. What this
            // does prove is the composition – the interface is drawn over the
            // model, in the same frame, and the model does not erase it.
            let marker = egui::Rect::from_min_size(egui::pos2(16.0, 16.0), egui::vec2(180.0, 28.0));
            ui.painter().rect_filled(
                marker,
                4.0,
                egui::Color32::from_rgba_unmultiplied(20, 20, 20, 200),
            );
        });
        self.egui_state
            .handle_platform_output(&self.window, output.platform_output);

        let (width, height) = frame.size();
        let descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: self.egui.pixels_per_point(),
        };
        let primitives = self
            .egui
            .tessellate(output.shapes, self.egui.pixels_per_point());
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.egui_renderer
                    .update_texture(frame.device(), frame.queue(), *id, delta);
            }
        }

        let mut encoder = frame
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ferritecad interface"),
            });
        self.egui_renderer.update_buffers(
            frame.device(),
            frame.queue(),
            &mut encoder,
            &primitives,
            &descriptor,
        );
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ferritecad interface pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: frame.view(),
                        depth_slice: None,
                        resolve_target: None,
                        // Load, never clear: the model is already in there.
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.egui_renderer
                .render(&mut pass, &primitives, &descriptor);
        }
        frame.queue().submit(Some(encoder.finish()));

        for id in &output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
        frame.present();
        Ok(())
    }
}

/// Turns one window event into what this application means by it.
///
/// Small and obvious on purpose: everything with a decision in it is on the
/// other side of this function, where it can be tested.
fn translate(event: &WindowEvent) -> Vec<ViewportEvent> {
    match event {
        WindowEvent::CursorMoved { position, .. } => vec![ViewportEvent::PointerMoved {
            x: position.x as f32,
            y: position.y as f32,
        }],
        WindowEvent::MouseInput { state, button, .. } => {
            let Some(button) = button_of(*button) else {
                return Vec::new();
            };
            vec![match state {
                ElementState::Pressed => ViewportEvent::PointerPressed(button),
                ElementState::Released => ViewportEvent::PointerReleased(button),
            }]
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let amount = match delta {
                MouseScrollDelta::LineDelta(_, lines) => *lines,
                // A trackpad reports pixels, which are a much finer unit than
                // a wheel's notches; scaled so both feel like the same gesture.
                MouseScrollDelta::PixelDelta(position) => position.y as f32 / 40.0,
            };
            vec![ViewportEvent::Wheel { delta: amount }]
        }
        WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
            named_view(&event.logical_key)
                .map(|view| vec![ViewportEvent::Look(view)])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

fn button_of(button: MouseButton) -> Option<PointerButton> {
    match button {
        MouseButton::Left => Some(PointerButton::Primary),
        MouseButton::Middle => Some(PointerButton::Middle),
        MouseButton::Right => Some(PointerButton::Secondary),
        _ => None,
    }
}

/// The number keys a drawing office would expect.
fn named_view(key: &Key) -> Option<StandardView> {
    let Key::Character(text) = key else {
        return match key {
            Key::Named(NamedKey::Home) => Some(StandardView::Isometric),
            _ => None,
        };
    };
    match text.as_str() {
        "1" => Some(StandardView::Front),
        "2" => Some(StandardView::Back),
        "3" => Some(StandardView::Left),
        "4" => Some(StandardView::Right),
        "5" => Some(StandardView::Top),
        "6" => Some(StandardView::Bottom),
        "7" => Some(StandardView::Isometric),
        _ => None,
    }
}

/// A box, so there is something with depth and orientation to turn around.
fn fixed_scene() -> Result<RenderSnapshot> {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&box_mesh(30.0, 20.0, 10.0))?;
    builder.place(mesh, None, &Transform::IDENTITY, [0.35, 0.55, 0.85])?;
    builder.place(
        mesh,
        None,
        &Transform::from_translation(Vec3::new(45.0, 0.0, 0.0)?)?,
        [0.85, 0.45, 0.25],
    )?;
    Ok(builder.build())
}

/// An axis-aligned box centred on the origin, with flat-shaded faces.
fn box_mesh(x: f32, y: f32, z: f32) -> Mesh {
    let (hx, hy, hz) = (x * 0.5, y * 0.5, z * 0.5);
    let corners = [
        [-hx, -hy, -hz],
        [hx, -hy, -hz],
        [hx, hy, -hz],
        [-hx, hy, -hz],
        [-hx, -hy, hz],
        [hx, -hy, hz],
        [hx, hy, hz],
        [-hx, hy, hz],
    ];
    let faces = [
        ([0, 3, 2, 1], [0.0, 0.0, -1.0]),
        ([4, 5, 6, 7], [0.0, 0.0, 1.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
        ([2, 3, 7, 6], [0.0, 1.0, 0.0]),
        ([1, 2, 6, 5], [1.0, 0.0, 0.0]),
        ([3, 0, 4, 7], [-1.0, 0.0, 0.0]),
    ];

    // Vertices are not shared between faces: a box drawn with shared corners
    // would have to average its normals and would look like a ball.
    let mut mesh = Mesh::default();
    let shape = ShapeHandle::new(SessionId::new(), 1);
    for (face, (indices, normal)) in faces.iter().enumerate() {
        let first = mesh.vertex_count() as u32;
        for index in indices {
            mesh.positions.extend_from_slice(&corners[*index]);
            mesh.normals.extend_from_slice(normal);
        }
        mesh.indices
            .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
        mesh.faces.push(MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, face as u64),
            first_index: face as u32 * 6,
            index_count: 6,
        });
    }
    mesh
}
