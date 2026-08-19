// SPDX-License-Identifier: MIT
//! The device, the targets and one pass over a snapshot.

use std::sync::Arc;

use ferritecad_types::{CadError, Result};
use ferritecad_viewport::{
    Camera, EdgePickId, FacePickId, Hovered, Marked, PickId, RenderSnapshot, VERTEX_FLOATS,
    Visibility,
};
use wgpu::util::DeviceExt as _;

/// Linear, not sRGB. The snapshot's colours are linear because that is what the
/// importer read out of the file, and a target that encoded on write would make
/// the bytes read back a statement about a transfer function rather than about
/// what was drawn.
pub const COLOUR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// One unsigned integer per pixel: an identity is not a colour, and storing it
/// in one would mean packing it into channels and hoping nothing filters it.
pub const PICK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;

/// Where a face identity is written, offscreen and nowhere else.
///
/// The same format as the identities beside it, and drawn in the same pass so
/// the two answers about one pixel are answers about the same triangle.
pub const FACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;

/// Where a topological edge identity is written, offscreen and nowhere else.
///
/// Its own target rather than a fourth channel of the pass beside it. An edge
/// is drawn over the surface it belongs to, so a pixel has both an edge and a
/// face, and one attachment cannot hold two answers. Cleared to zero, which is
/// what a pixel with no edge on it reads as.
pub const EDGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// One vertex of the expanded edge stream.
///
/// Sixteen bytes: a position and the identity of the edge this end belongs to.
/// See `vertex_edge` in the shader for why the identity travels with a vertex
/// of its own rather than beside the model's positions.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct EdgeVertex {
    position: [f32; 3],
    edge: u32,
}

/// Per-draw uniform data, as the shader declares it.
///
/// Ninety-six bytes, and the shader's `Draw` must come to the same number: a
/// uniform binding whose size disagrees with the shader's view of it is
/// rejected when the pipeline is created, which is at least a loud place to
/// find out. See the note in `shader.wgsl` about why the padding is three
/// scalars and not a `vec3<u32>`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawUniform {
    transform: [f32; 16],
    colour: [f32; 4],
    pick: u32,
    padding: [u32; 3],
}

/// What every draw in one frame is drawn against.
///
/// The selection lives here rather than in the per-draw uniforms, and that is
/// what makes every placement of one definition light up together: they carry
/// the same identity, so one comparison in the shader covers all of them. It
/// also means that selecting something rewrites nothing that was uploaded
/// once.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GlobalsUniform {
    view_projection: [f32; 16],
    /// The identity to draw as selected, or zero for none. Zero is what the
    /// background reads as, and no definition is ever numbered zero.
    selected: u32,
    /// The face to draw as selected, or zero. Never set beside `selected`:
    /// choosing a face is choosing that face and not the part around it.
    selected_face: u32,
    /// The face the pointer is over, or zero. A face of the picture, so the
    /// same face is marked in every placement of its definition.
    hovered_face: u32,
    /// The identity the pointer is over, or zero. Kept apart from the
    /// selection because they are different states and a person must be able
    /// to tell which is which: one is a decision and the other is a question.
    hovered: u32,
    /// The topological edge the pointer is over, or zero.
    hovered_edge: u32,
    /// Three scalars, written down rather than left to a compiler.
    ///
    /// A `mat4x4` gives this struct sixteen-byte alignment in WGSL, so its
    /// size is rounded up to a multiple of sixteen there. Five `u32` after the
    /// matrix is 84 bytes in Rust and 96 in WGSL, and a uniform binding whose
    /// size disagrees with the shader's view of it is a mismatch one backend
    /// may forgive and another will not. Padding to 96 on both sides makes the
    /// agreement explicit, and the assertion below makes it checked.
    padding: [u32; 3],
}

// Ninety-six bytes, matching `Globals` in `shader.wgsl` exactly. A change to
// either that forgets the other stops the build here rather than at whichever
// driver notices first.
const _: () = assert!(std::mem::size_of::<GlobalsUniform>() == 96);

/// What a grid pass needs to know, and all it is allowed to know.
///
/// No model, no selection and no identity: a backdrop that could see any of
/// those could be made to depend on them.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GridUniform {
    view_projection: [f32; 16],
    minor: f32,
    major: f32,
    extent: f32,
    half_lines: u32,
}

/// Distinguishes one renderer from another.
///
/// The same idea as the kernel's `SessionId`, for the same reason: a buffer
/// belongs to the device that allocated it, and handing it to another one is a
/// lifetime mistake that would otherwise surface as a driver error far from its
/// cause. Every renderer gets a value no earlier one in this process has used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RendererId(u64);

impl RendererId {
    pub(crate) fn next() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

impl std::fmt::Display for RendererId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "renderer#{}", self.0)
    }
}

/// A device, a pipeline and the buffers a frame is drawn with.
#[derive(Debug)]
pub struct Renderer {
    id: RendererId,
    /// Kept so a window's surface can be asked what it supports, and so the
    /// adapter that was checked for compatibility is the one that draws.
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The offscreen pipeline, which writes colour and identities together.
    pipeline: wgpu::RenderPipeline,
    /// The offscreen pipeline that draws where faces stop. Built beside the
    /// fill, from the same shader and layout, so the two cannot disagree
    /// about where the model is.
    line_pipeline: wgpu::RenderPipeline,
    /// One pipeline per surface format met so far. A window chooses its own
    /// format and a pipeline is built for the format it is drawn into, not for
    /// the one this crate would have preferred.
    surface_pipelines: std::collections::HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    /// The same linework for a window format, learned when a window is.
    line_surface_pipelines: std::collections::HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    /// Draws the topological edges into their own identity target. Offscreen
    /// only: a window has no such attachment and is not asked for one.
    edge_pipeline: wgpu::RenderPipeline,
    /// Marks the edge under the pointer, offscreen.
    edge_mark_pipeline: wgpu::RenderPipeline,
    /// The same mark for a window format, learned when a window is.
    edge_mark_surface_pipelines:
        std::collections::HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    layout: wgpu::BindGroupLayout,
    /// The camera's matrix, rewritten each frame rather than reallocated.
    /// Shared by every prepared snapshot this renderer owns, which is why
    /// drawing takes `&mut self`: two frames in flight would race on it.
    globals: wgpu::Buffer,
    /// The alignment a dynamic uniform offset must be a multiple of, which is a
    /// property of the device and not a constant anyone may assume.
    draw_stride: u64,
    geometry_uploads: u64,
    /// The backdrop, kept beside the model's pipelines and built the same way
    /// for both targets so the window and the offscreen path cannot draw
    /// different grids.
    grid_shader: wgpu::ShaderModule,
    grid_pipeline_layout: wgpu::PipelineLayout,
    grid_pipeline: wgpu::RenderPipeline,
    grid_surface_pipelines: std::collections::HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    grid_globals: wgpu::Buffer,
    grid_bindings: wgpu::BindGroup,
}

impl Renderer {
    /// Opens a device, or explains that there is none.
    ///
    /// A headless machine with no adapter is an ordinary condition, not a
    /// defect: the failure is returned so a caller can skip rather than
    /// panicking somewhere less obvious.
    pub fn new() -> Result<Self> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|error| {
                CadError::unsupported(format!(
                    "no graphics adapter is available to draw with: {error}"
                ))
            })?;
        Self::on(adapter)
    }

    /// Opens a device on an adapter that can present to `surface`.
    ///
    /// The distinction matters and is not cosmetic. A machine may hold several
    /// adapters, and the first one that can compute is not always one that can
    /// put pixels in a particular window – a discrete card with no connection
    /// to the display the window is on, or a software adapter with no
    /// presentation support at all. Asking for any adapter and then handing it
    /// a surface fails later, inside the driver, with a message about neither.
    ///
    /// The surface must outlive nothing here: it is borrowed only for the
    /// question. Give it to [`WindowSurface::new`][crate::WindowSurface::new]
    /// afterwards.
    pub fn for_surface(instance: &wgpu::Instance, surface: &wgpu::Surface<'_>) -> Result<Self> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(surface),
            ..Default::default()
        }))
        .map_err(|error| {
            CadError::unsupported(format!(
                "no graphics adapter can present to this window: {error}"
            ))
        })?;
        Self::on(adapter)
    }

    fn on(adapter: wgpu::Adapter) -> Result<Self> {
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ferritecad viewport"),
            ..Default::default()
        }))
        .map_err(|error| {
            CadError::rendering_because("a graphics adapter was found but refused a device", error)
        })?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ferritecad viewport shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ferritecad viewport bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    // Both stages: the vertex stage projects with it, and the
                    // fragment stage asks it what is selected.
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        // One buffer holding every draw, walked by offset. The
                        // alternative is a bind group per draw, which is more
                        // objects to keep in step for no gain at this size.
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<DrawUniform>() as u64
                        ),
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ferritecad viewport pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        // Watched, because a device that refuses a pipeline reports it through
        // the uncaptured-error handler, which panics the process. Validation
        // is our rendering defect and an internal failure is an unsupported
        // driver; both are answers a caller can act on rather than a crash.
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let pipeline = build_pipeline(&device, &shader, &pipeline_layout, COLOUR_FORMAT, true);

        let grid_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ferritecad grid shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("grid.wgsl").into()),
        });
        let grid_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ferritecad grid bindings"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let grid_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ferritecad grid pipeline layout"),
            bind_group_layouts: &[Some(&grid_layout)],
            immediate_size: 0,
        });
        let grid_globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ferritecad grid globals"),
            size: std::mem::size_of::<GridUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let grid_bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ferritecad grid bind group"),
            layout: &grid_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: grid_globals.as_entire_binding(),
            }],
        });
        let grid_pipeline = build_grid_pipeline(
            &device,
            &grid_shader,
            &grid_pipeline_layout,
            COLOUR_FORMAT,
            true,
        );
        let line_pipeline =
            build_line_pipeline(&device, &shader, &pipeline_layout, COLOUR_FORMAT, true);
        let edge_pipeline = build_edge_pipeline(&device, &shader, &pipeline_layout);
        let edge_mark_pipeline =
            build_edge_mark_pipeline(&device, &shader, &pipeline_layout, COLOUR_FORMAT);
        // Pop both even when the inner scope caught an error. Leaving the
        // outer one installed would make a later, unrelated device error look
        // as though it belonged to this build.
        let internal_refusal = pollster::block_on(internal.pop()).map(|error| error.to_string());
        let validation_refusal =
            pollster::block_on(validation.pop()).map(|error| error.to_string());
        pipeline_refusal(validation_refusal, internal_refusal)?;

        let draw_stride = align_to(
            std::mem::size_of::<DrawUniform>() as u64,
            u64::from(device.limits().min_uniform_buffer_offset_alignment),
        );

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ferritecad viewport globals"),
            size: std::mem::size_of::<GlobalsUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            id: RendererId::next(),
            adapter,
            device,
            queue,
            pipeline,
            line_pipeline,
            surface_pipelines: std::collections::HashMap::new(),
            line_surface_pipelines: std::collections::HashMap::new(),
            edge_pipeline,
            edge_mark_pipeline,
            edge_mark_surface_pipelines: std::collections::HashMap::new(),
            shader,
            pipeline_layout,
            layout,
            globals,
            draw_stride,
            geometry_uploads: 0,
            grid_shader,
            grid_pipeline_layout,
            grid_pipeline,
            grid_surface_pipelines: std::collections::HashMap::new(),
            grid_globals,
            grid_bindings,
        })
    }

    pub fn id(&self) -> RendererId {
        self.id
    }

    /// The adapter this renderer draws with.
    ///
    /// A surface must be asked what *this* adapter supports, not what some
    /// other one would have.
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Prepares the backdrop, and says whether there is one to draw.
    ///
    /// A picture with nothing in it gets no grid: an empty document is empty,
    /// and drawing a floor under nothing would be inventing content for it. A
    /// camera with no screen gets none either, for want of a scale to choose
    /// spacing against.
    ///
    /// The plan comes from `ferritecad-viewport`, so the offscreen path and
    /// the window ask the same arithmetic the same question.
    fn write_grid(&self, camera: &Camera, snapshot: &RenderSnapshot) -> bool {
        if snapshot.bounds().is_none() {
            return false;
        }
        let Some(plan) = ferritecad_viewport::grid_plan(camera) else {
            return false;
        };

        self.queue.write_buffer(
            &self.grid_globals,
            0,
            bytemuck::bytes_of(&GridUniform {
                view_projection: camera.view_projection(),
                minor: plan.minor,
                major: plan.major,
                extent: plan.extent,
                half_lines: ferritecad_viewport::HALF_LINES,
            }),
        );
        true
    }

    /// Draws the backdrop into a pass that has just cleared.
    ///
    /// Before the model and without writing depth, which is what makes the
    /// model win wherever it draws: a part below the plane is drawn over the
    /// lines rather than hidden by them.
    fn draw_grid(
        pass: &mut wgpu::RenderPass<'_>,
        pipeline: &wgpu::RenderPipeline,
        bindings: &wgpu::BindGroup,
    ) {
        let per_axis = 2 * ferritecad_viewport::HALF_LINES + 1;
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bindings, &[]);
        // Two vertices per line, two axes' worth of lines, and no buffer: the
        // shader derives every position from the vertex number.
        pass.draw(0..(per_axis * 2 * 2), 0..1);
    }

    /// Writes what every draw in the coming frame is drawn against.
    ///
    /// A selection from another snapshot selects nothing. The identity a pick
    /// carries is only meaningful inside the snapshot that issued it, and the
    /// same number in another one names a different definition – so the
    /// snapshot about to be drawn is asked, and an answer it does not
    /// recognise becomes no selection at all.
    fn write_globals(
        &self,
        camera: &Camera,
        prepared: &PreparedSnapshot,
        selected: Marked,
        hovered: Hovered,
    ) {
        // Every identity asked of the picture about to be drawn, and by the
        // same question. A number that named a definition of some other
        // picture would otherwise light up whichever one occupies it here.
        let snapshot = prepared.snapshot();
        // The rule, asked once and before the raw values shadow their marks:
        // an edge the choice already covers is not marked, so the shader is
        // never told to mark one.
        let marked_edge =
            Self::marked_edge(snapshot, selected, hovered).map_or(0, |edge| edge.to_raw());
        let (selected, selected_face) = match selected.known_to(snapshot) {
            Marked::Nothing => (PickId::NOTHING.to_raw(), FacePickId::NOTHING.to_raw()),
            Marked::Definition(pick) => (pick.to_raw(), FacePickId::NOTHING.to_raw()),
            Marked::Face(face) => (PickId::NOTHING.to_raw(), face.to_raw()),
        };
        let (hovered, hovered_face, hovered_edge) = match hovered.known_to(snapshot) {
            Hovered::Nothing => (
                PickId::NOTHING.to_raw(),
                FacePickId::NOTHING.to_raw(),
                EdgePickId::NOTHING.to_raw(),
            ),
            Hovered::Definition(pick) => (
                pick.to_raw(),
                FacePickId::NOTHING.to_raw(),
                EdgePickId::NOTHING.to_raw(),
            ),
            Hovered::Face(face) => (
                PickId::NOTHING.to_raw(),
                face.to_raw(),
                EdgePickId::NOTHING.to_raw(),
            ),
            Hovered::Edge(_) => (
                PickId::NOTHING.to_raw(),
                FacePickId::NOTHING.to_raw(),
                marked_edge,
            ),
        };
        self.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&GlobalsUniform {
                view_projection: camera.view_projection(),
                selected,
                selected_face,
                hovered_face,
                hovered,
                hovered_edge,
                padding: [0; 3],
            }),
        );
    }

    /// The largest texture this device will make, which is also the largest
    /// window it can be asked to fill.
    pub fn max_texture_dimension(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    /// Draws a prepared snapshot into a view somebody else owns.
    ///
    /// Used for a window, whose texture comes from its surface and is presented
    /// rather than read back. The geometry is the geometry that was prepared;
    /// nothing is uploaded here.
    ///
    /// Identities are not written on this path. A window frame is drawn many
    /// times a second and a pick is asked for when somebody clicks, so paying
    /// for the second on every one of the first would be paying continuously
    /// for something wanted occasionally. Picking renders offscreen, through
    /// [`Self::render`], at the same camera.
    // Seven of these are one thing each: what to draw, from where, what is
    // chosen, where to put it, and the three facts about that target. A struct
    // built at the one call site would hide the count rather than reduce it.
    #[expect(clippy::too_many_arguments, reason = "see above")]
    pub(crate) fn draw_into(
        &mut self,
        prepared: &PreparedSnapshot,
        camera: &Camera,
        selected: Marked,
        hovered: Hovered,
        visibility: &Visibility,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.require_own(prepared)?;

        self.ensure_surface_pipeline(format)?;
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ferritecad viewport window depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());

        self.write_globals(camera, prepared, selected, hovered);
        let grid = self.write_grid(camera, prepared.snapshot());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ferritecad viewport window frame"),
            });
        {
            let pipeline = self
                .surface_pipelines
                .get(&format)
                .expect("the pipeline for this format was just ensured");
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ferritecad viewport window pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if grid {
                let grid_pipeline = self
                    .grid_surface_pipelines
                    .get(&format)
                    .expect("the grid pipeline for this format was just ensured");
                Self::draw_grid(&mut pass, grid_pipeline, &self.grid_bindings);
            }

            pass.set_pipeline(pipeline);
            Self::draw_model(&mut pass, prepared, visibility, self.draw_stride);

            // Last, over the surfaces they bound and behind anything nearer.
            let line_pipeline = self
                .line_surface_pipelines
                .get(&format)
                .expect("the line pipeline for this format was just ensured");
            pass.set_pipeline(line_pipeline);
            Self::draw_lines(&mut pass, prepared, visibility, self.draw_stride);

            // And the marked edge over all of it, from the same stream and
            // the same rule the readback uses. A window pass is colour-only
            // already, so the mark needs no pass of its own here.
            if Self::marked_edge(prepared.snapshot(), selected, hovered).is_some() {
                let mark_pipeline = self
                    .edge_mark_surface_pipelines
                    .get(&format)
                    .expect("the edge mark pipeline for this format was just ensured");
                pass.set_pipeline(mark_pipeline);
                Self::draw_edges(&mut pass, prepared, visibility, self.draw_stride);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    /// Refuses buffers that live on another device.
    ///
    /// One definition, used by the offscreen path and the window one alike. A
    /// second copy would be a second thing to keep in step, and the one that
    /// drifted would be whichever is harder to reach from a test.
    fn require_own(&self, prepared: &PreparedSnapshot) -> Result<()> {
        if prepared.renderer != self.id {
            return Err(CadError::rendering(format!(
                "this snapshot was prepared by {} and cannot be drawn by {}: its buffers \
                 belong to the other device",
                prepared.renderer, self.id
            )));
        }
        Ok(())
    }

    /// Hands a drawn surface texture to the compositor.
    /// Draws every visible definition of a prepared picture, in order.
    ///
    /// One loop, called by the window path and by the offscreen one, so a
    /// definition that is not drawn in a window cannot still be drawn into the
    /// identities a click reads. A hidden definition is skipped rather than
    /// dimmed: what is not drawn writes no colour, no identity and no face,
    /// which is the whole of what hiding means here.
    fn draw_model(
        pass: &mut wgpu::RenderPass<'_>,
        prepared: &PreparedSnapshot,
        visibility: &Visibility,
        stride: u64,
    ) {
        let hidden = visibility.hidden_in(prepared.snapshot());
        // In the order the snapshot lists them. A renderer that sorted to save
        // state changes would make two frames of one model differ.
        for (index, item) in prepared.snapshot.draws().iter().enumerate() {
            if hidden.get(item.mesh).copied().unwrap_or(false) {
                continue;
            }
            let mesh = &prepared.meshes[item.mesh];
            if mesh.index_count == 0 {
                continue;
            }
            pass.set_bind_group(0, &prepared.bindings, &[(index as u64 * stride) as u32]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_vertex_buffer(1, mesh.faces.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        }
    }

    /// Draws where every visible definition's faces stop, in the same order.
    ///
    /// The same visibility, the same placements and the same vertices as the
    /// fill: a definition that is not drawn has no boundary drawn either, and
    /// a definition placed twice has its boundary drawn twice. Called after
    /// the fill so that a line is over the surface it belongs to and behind
    /// anything nearer.
    fn draw_lines(
        pass: &mut wgpu::RenderPass<'_>,
        prepared: &PreparedSnapshot,
        visibility: &Visibility,
        stride: u64,
    ) {
        let hidden = visibility.hidden_in(prepared.snapshot());
        for (index, item) in prepared.snapshot.draws().iter().enumerate() {
            if hidden.get(item.mesh).copied().unwrap_or(false) {
                continue;
            }
            let mesh = &prepared.meshes[item.mesh];
            if mesh.line_index_count == 0 {
                continue;
            }
            pass.set_bind_group(0, &prepared.bindings, &[(index as u64 * stride) as u32]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_vertex_buffer(1, mesh.faces.slice(..));
            pass.set_index_buffer(mesh.lines.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.line_index_count, 0, 0..1);
        }
    }

    /// Draws every visible definition's topological edges, in the same order.
    ///
    /// The same visibility and the same placements as the fill, for the same
    /// reasons: a definition that is not drawn leaves no edge identity behind,
    /// and a definition placed twice answers with the same edge identities in
    /// both places, because an edge belongs to a definition and not to an
    /// occurrence of one.
    ///
    /// One draw per placement rather than one per edge. What distinguishes the
    /// edges is in the vertex stream, so the whole of a definition's linework
    /// is one call however many edges it has.
    fn draw_edges(
        pass: &mut wgpu::RenderPass<'_>,
        prepared: &PreparedSnapshot,
        visibility: &Visibility,
        stride: u64,
    ) {
        let hidden = visibility.hidden_in(prepared.snapshot());
        for (index, item) in prepared.snapshot.draws().iter().enumerate() {
            if hidden.get(item.mesh).copied().unwrap_or(false) {
                continue;
            }
            let mesh = &prepared.meshes[item.mesh];
            if mesh.edge_vertex_count == 0 {
                continue;
            }
            pass.set_bind_group(0, &prepared.bindings, &[(index as u64 * stride) as u32]);
            pass.set_vertex_buffer(0, mesh.edges.slice(..));
            pass.draw(0..mesh.edge_vertex_count, 0..1);
        }
    }

    /// Which edge, if any, this frame marks under the pointer.
    ///
    /// A question loses to a decision wherever the two are about the same
    /// geometry, and this is the whole of that rule. A part that has been
    /// chosen keeps its chosen look, so an edge of it is not marked; a face
    /// that has been chosen keeps its chosen look, so an edge bounding it is
    /// not marked either. An edge of the same part that does not bound the
    /// chosen face is marked, because nothing about it overlaps the choice,
    /// and a choice made in another part does not reach here at all.
    ///
    /// Adjacency is asked of the picture, which reads it out of the packed
    /// partition. It is a property of the edge rather than of either side of
    /// it, so the answer cannot depend on which of two coincident face-side
    /// lines happened to be drawn last.
    fn marked_edge(
        snapshot: &RenderSnapshot,
        selected: Marked,
        hovered: Hovered,
    ) -> Option<EdgePickId> {
        let Hovered::Edge(edge) = hovered.known_to(snapshot) else {
            return None;
        };
        match selected.known_to(snapshot) {
            Marked::Nothing => Some(edge),
            Marked::Definition(pick) => {
                (snapshot.definition(pick) != snapshot.definition_of_edge(edge)).then_some(edge)
            }
            Marked::Face(face) => (!snapshot.edge_bounds_face(edge, face)).then_some(edge),
        }
    }

    pub(crate) fn present(&self, texture: wgpu::SurfaceTexture) {
        self.queue.present(texture);
    }

    /// Builds the pipeline for a window format, once per format met.
    fn ensure_surface_pipeline(&mut self, format: wgpu::TextureFormat) -> Result<()> {
        let needs_model = !self.surface_pipelines.contains_key(&format);
        let needs_grid = !self.grid_surface_pipelines.contains_key(&format);
        let needs_lines = !self.line_surface_pipelines.contains_key(&format);
        let needs_marks = !self.edge_mark_surface_pipelines.contains_key(&format);
        if !needs_model && !needs_grid && !needs_lines && !needs_marks {
            return Ok(());
        }

        // Surface formats are learned only after a window exists, so these
        // pipelines are necessarily lazy. Watch them just like the offscreen
        // pipelines: a validation failure is our rendering defect, while an
        // internal driver refusal is an unsupported adapter. Neither belongs
        // in wgpu's uncaptured-error handler, which would panic the process.
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let internal = self.device.push_error_scope(wgpu::ErrorFilter::Internal);
        let model = needs_model.then(|| {
            build_pipeline(
                &self.device,
                &self.shader,
                &self.pipeline_layout,
                format,
                // No identity target: see `draw_into`.
                false,
            )
        });
        let grid = needs_grid.then(|| {
            build_grid_pipeline(
                &self.device,
                &self.grid_shader,
                &self.grid_pipeline_layout,
                format,
                false,
            )
        });
        let lines = needs_lines.then(|| {
            build_line_pipeline(
                &self.device,
                &self.shader,
                &self.pipeline_layout,
                format,
                false,
            )
        });
        let marks = needs_marks.then(|| {
            build_edge_mark_pipeline(&self.device, &self.shader, &self.pipeline_layout, format)
        });
        let internal_refusal = pollster::block_on(internal.pop()).map(|error| error.to_string());
        let validation_refusal =
            pollster::block_on(validation.pop()).map(|error| error.to_string());
        pipeline_refusal(validation_refusal, internal_refusal)?;

        // Publish none until every requested pipeline was accepted. A retry
        // after a refusal therefore starts from one coherent state.
        if let Some(model) = model {
            self.surface_pipelines.insert(format, model);
        }
        if let Some(grid) = grid {
            self.grid_surface_pipelines.insert(format, grid);
        }
        if let Some(lines) = lines {
            self.line_surface_pipelines.insert(format, lines);
        }
        if let Some(marks) = marks {
            self.edge_mark_surface_pipelines.insert(format, marks);
        }
        Ok(())
    }

    /// How many mesh buffers this renderer has uploaded.
    ///
    /// Exposed so "the geometry is uploaded once" can be asserted from outside
    /// rather than believed. The kernel adapter offers `live_shape_count` for
    /// the same reason: an ownership claim nobody can check is a comment.
    pub fn geometry_uploads(&self) -> u64 {
        self.geometry_uploads
    }

    /// Uploads a snapshot's geometry and keeps it on the device.
    ///
    /// Everything that does not depend on the camera happens here and once:
    /// vertex and index buffers, the per-draw uniforms, and the bindings that
    /// tie them to this renderer's camera buffer. Drawing the result again
    /// costs a matrix write and a pass.
    ///
    /// The result belongs to this renderer. Another one will refuse it – see
    /// [`Self::render`].
    pub fn prepare(&mut self, snapshot: Arc<RenderSnapshot>) -> Result<PreparedSnapshot> {
        let draw_buffer_size = self.validate_snapshot(&snapshot)?;

        // Every draw's uniform data in one buffer, each at a device-aligned
        // offset. An empty snapshot still needs one stride of buffer, because a
        // zero-sized uniform buffer is not a thing a device will bind.
        let mut draw_bytes = Vec::new();
        draw_bytes
            .try_reserve_exact(draw_buffer_size)
            .map_err(|error| CadError::rendering_because("allocating draw uniforms", error))?;
        draw_bytes.resize(draw_buffer_size, 0);
        for (index, item) in snapshot.draws().iter().enumerate() {
            let uniform = DrawUniform {
                transform: item.transform,
                colour: item.colour,
                pick: item.pick.to_raw(),
                padding: [0; 3],
            };
            let at = index * self.draw_stride as usize;
            draw_bytes[at..at + std::mem::size_of::<DrawUniform>()]
                .copy_from_slice(bytemuck::bytes_of(&uniform));
        }
        let draws = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ferritecad viewport draws"),
                contents: &draw_bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ferritecad viewport bindings"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &draws,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<DrawUniform>() as u64),
                    }),
                },
            ],
        });

        // One vertex and one index buffer per packed mesh.
        let mut meshes = Vec::new();
        meshes
            .try_reserve_exact(snapshot.meshes().len())
            .map_err(|error| CadError::rendering_because("recording mesh buffers", error))?;
        for (index, mesh) in snapshot.meshes().iter().enumerate() {
            let vertices = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ferritecad viewport vertices"),
                    contents: bytemuck::cast_slice(mesh.vertices()),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let indices = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ferritecad viewport indices"),
                    contents: bytemuck::cast_slice(mesh.indices()),
                    usage: wgpu::BufferUsages::INDEX,
                });
            // Which face each vertex belongs to, beside the vertices rather
            // than woven into them: the positions and normals keep the layout
            // every other part of this crate agrees on, and a face is one more
            // buffer whose contents were decided when the picture was packed.
            let faces = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ferritecad viewport faces"),
                    contents: bytemuck::cast_slice(mesh.faces_of_vertices()),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            // Where the faces stop, decided when the picture was packed and
            // uploaded here once with everything else. An empty buffer is not
            // a buffer, so a mesh with no boundary carries none.
            let lines = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ferritecad viewport face boundaries"),
                    contents: bytemuck::cast_slice(if mesh.line_indices().is_empty() {
                        &[0u32]
                    } else {
                        mesh.line_indices()
                    }),
                    usage: wgpu::BufferUsages::INDEX,
                });
            // The topological edges, expanded so that each segment carries
            // its own two ends. Built here, once, from the partition the
            // picture was packed with: no position is matched against another,
            // and nothing the kernel called an edge is uploaded — only the
            // number this picture gave it.
            let edge_vertices = edge_stream(&snapshot, index, mesh)?;
            let edge_vertex_count = edge_vertices.len() as u32;
            let edges = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ferritecad viewport edge identities"),
                    contents: if edge_vertices.is_empty() {
                        // A zero-sized buffer is not a buffer. Nothing draws
                        // from this one: the count above is what decides.
                        bytemuck::bytes_of(&EdgeVertex {
                            position: [0.0; 3],
                            edge: 0,
                        })
                    } else {
                        bytemuck::cast_slice(&edge_vertices)
                    },
                    usage: wgpu::BufferUsages::VERTEX,
                });
            meshes.push(GpuMesh {
                vertices,
                faces,
                indices,
                index_count: mesh.indices().len() as u32,
                lines,
                line_index_count: mesh.line_indices().len() as u32,
                edges,
                edge_vertex_count,
            });
            self.geometry_uploads += 1;
        }

        Ok(PreparedSnapshot {
            renderer: self.id,
            snapshot,
            meshes,
            bindings,
        })
    }

    /// Draws a prepared snapshot through one camera and reads the result back.
    ///
    /// The snapshot travels into the returned frame, so what was drawn and what
    /// a pick is interpreted against cannot come apart.
    ///
    /// Only the camera changes between frames. The geometry was uploaded when
    /// the snapshot was prepared and is not touched here.
    pub fn render(
        &mut self,
        prepared: &PreparedSnapshot,
        camera: &Camera,
        selected: Marked,
        hovered: Hovered,
        visibility: &Visibility,
    ) -> Result<Frame> {
        self.require_own(prepared)?;

        let snapshot = Arc::clone(&prepared.snapshot);
        let (width, height) = (camera.width(), camera.height());
        if !camera.is_drawable() {
            // A minimised window, or the moment before a first layout. There is
            // nothing to draw into and nothing to read back, and that is an
            // answer rather than an error.
            return Ok(Frame {
                snapshot,
                width,
                height,
                colour: Vec::new(),
                picks: Vec::new(),
                faces: Vec::new(),
                edges: Vec::new(),
            });
        }

        let readback = self.validate_frame(width, height)?;

        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let colour = self.target("colour", extent, COLOUR_FORMAT);
        let pick = self.target("pick", extent, PICK_FORMAT);
        let face = self.target("face", extent, FACE_FORMAT);
        let edge = self.target("edge", extent, EDGE_FORMAT);
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ferritecad viewport depth"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        // The things that differ from frame to frame.
        self.write_globals(camera, prepared, selected, hovered);
        let grid = self.write_grid(camera, prepared.snapshot());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ferritecad viewport frame"),
            });

        {
            let colour_view = colour.create_view(&Default::default());
            let pick_view = pick.create_view(&Default::default());
            let face_view = face.create_view(&Default::default());
            let depth_view = depth.create_view(&Default::default());

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ferritecad viewport pass"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &colour_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &pick_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            // Zero is what nothing reads as, which is why a
                            // definition's identity is its index plus one.
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &face_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            // Zero again, and for the same reason: a face's
                            // identity is its number within the picture plus
                            // one, so nowhere reads as some face.
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        // Kept, because the edge pass that follows tests
                        // against exactly this depth. Discarding it would make
                        // that pass answer for edges the model hides.
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if grid {
                // First, and without writing depth: what follows covers it
                // wherever it draws, on either side of the plane.
                Self::draw_grid(&mut pass, &self.grid_pipeline, &self.grid_bindings);
            }

            pass.set_pipeline(&self.pipeline);
            Self::draw_model(&mut pass, prepared, visibility, self.draw_stride);

            // Last, over the surfaces they bound and behind anything nearer.
            pass.set_pipeline(&self.line_pipeline);
            Self::draw_lines(&mut pass, prepared, visibility, self.draw_stride);
        }

        if Self::marked_edge(prepared.snapshot(), selected, hovered).is_some() {
            // The one edge under the pointer, over the picture and nothing
            // else. Its own pass, with the colour and the depth loaded: the
            // identity targets are not attached, so marking an edge cannot
            // change what any pixel *is*. Skipped entirely when there is no
            // edge to mark, so a picture with no question in it is drawn by
            // exactly the passes it was drawn by before.
            let colour_view = colour.create_view(&Default::default());
            let depth_view = depth.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ferritecad viewport edge mark pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &colour_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        // The model's own depth, kept for the identity pass
                        // that follows. Nothing here writes it.
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.edge_mark_pipeline);
            Self::draw_edges(&mut pass, prepared, visibility, self.draw_stride);
        }

        {
            // Which topological edge is under each pixel, in a pass of its own
            // against the depth the picture was drawn to.
            //
            // A separate pass rather than a fourth attachment on the one
            // above. The grid, the fill and the face boundaries would all have
            // to declare a target they never write, and "the backdrop leaves
            // the edge target alone" would then be a write mask somebody could
            // change. Here they are simply not drawn into it.
            //
            // The colour, definition and face targets are not attached at all,
            // so nothing in this pass can reach them.
            let edge_view = edge.create_view(&Default::default());
            let depth_view = depth.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ferritecad viewport edge identity pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &edge_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Zero, which is what a pixel with no edge on it reads
                        // as: an edge's identity is its number within the
                        // picture plus one.
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        // Loaded, not cleared: this is the model's own depth,
                        // which is what stops an edge answering through the
                        // part in front of it. Discarded afterwards, because
                        // nothing else reads it and this pass wrote none of it.
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.edge_pipeline);
            Self::draw_edges(&mut pass, prepared, visibility, self.draw_stride);
        }

        let colour_read = self.readback(&mut encoder, &colour, extent, readback);
        let pick_read = self.readback(&mut encoder, &pick, extent, readback);
        let face_read = self.readback(&mut encoder, &face, extent, readback);
        let edge_read = self.readback(&mut encoder, &edge, extent, readback);
        self.queue.submit(Some(encoder.finish()));

        let colour = self.take(colour_read, height, readback)?;
        let picks = self.take(pick_read, height, readback)?;
        let faces = self.take(face_read, height, readback)?;
        let edges = self.take(edge_read, height, readback)?;

        Ok(Frame {
            snapshot,
            width,
            height,
            colour,
            picks: unpack(&picks),
            faces: unpack(&faces),
            edges: unpack(&edges),
        })
    }

    fn target(
        &self,
        name: &str,
        extent: wgpu::Extent3d,
        format: wgpu::TextureFormat,
    ) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(name),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    fn validate_frame(&self, width: u32, height: u32) -> Result<ReadbackLayout> {
        let limits = self.device.limits();
        let maximum = limits.max_texture_dimension_2d;
        if width > maximum || height > maximum {
            return Err(CadError::input(format!(
                "viewport {width}x{height} exceeds this device's {maximum}x{maximum} texture limit"
            )));
        }

        let row = u64::from(width)
            .checked_mul(4)
            .ok_or_else(|| CadError::input("viewport row size overflows its number format"))?;
        let padded = align_to(row, u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT));
        let padded = u32::try_from(padded)
            .map_err(|_| CadError::input("viewport padded row does not fit the graphics API"))?;
        let buffer_size = u64::from(padded)
            .checked_mul(u64::from(height))
            .ok_or_else(|| CadError::input("viewport readback size overflows its number format"))?;
        if buffer_size > limits.max_buffer_size {
            return Err(CadError::input(format!(
                "viewport readback needs {buffer_size} bytes, exceeding this device's {}-byte buffer limit",
                limits.max_buffer_size
            )));
        }
        let tight_size = row
            .checked_mul(u64::from(height))
            .and_then(|size| usize::try_from(size).ok())
            .ok_or_else(|| CadError::input("viewport does not fit in host address space"))?;

        Ok(ReadbackLayout {
            row: row as usize,
            padded,
            buffer_size,
            tight_size,
        })
    }

    fn validate_snapshot(&self, snapshot: &RenderSnapshot) -> Result<usize> {
        let limits = self.device.limits();
        let draws = snapshot.draws().len().max(1);
        let buffer_size = self.draw_stride.checked_mul(draws as u64).ok_or_else(|| {
            CadError::input("draw uniform buffer size overflows its number format")
        })?;
        if buffer_size > limits.max_buffer_size {
            return Err(CadError::input(format!(
                "draw uniforms need {buffer_size} bytes, exceeding this device's {}-byte buffer limit",
                limits.max_buffer_size
            )));
        }
        if let Some(last) = snapshot.draws().len().checked_sub(1) {
            let offset = self
                .draw_stride
                .checked_mul(last as u64)
                .ok_or_else(|| CadError::input("dynamic uniform offset overflows"))?;
            if offset > u64::from(u32::MAX) {
                return Err(CadError::input(
                    "draw uniforms exceed the u32 dynamic-offset address space",
                ));
            }
        }

        for (index, mesh) in snapshot.meshes().iter().enumerate() {
            for (what, elements, element_size) in [
                ("vertex", mesh.vertices().len(), std::mem::size_of::<f32>()),
                ("index", mesh.indices().len(), std::mem::size_of::<u32>()),
            ] {
                let bytes = elements.checked_mul(element_size).ok_or_else(|| {
                    CadError::input(format!("mesh {index} {what} buffer size overflows"))
                })?;
                if bytes as u64 > limits.max_buffer_size {
                    return Err(CadError::input(format!(
                        "mesh {index} {what} buffer needs {bytes} bytes, exceeding this device's {}-byte limit",
                        limits.max_buffer_size
                    )));
                }
            }
        }

        usize::try_from(buffer_size)
            .map_err(|_| CadError::input("draw uniform buffer does not fit in host address space"))
    }

    /// Copies a target into a buffer a host can map.
    ///
    /// Rows are padded to the alignment a copy demands, which is why the result
    /// is unpacked again rather than used as it lands.
    fn readback(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        extent: wgpu::Extent3d,
        layout: ReadbackLayout,
    ) -> wgpu::Buffer {
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ferritecad viewport readback"),
            size: layout.buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(layout.padded),
                    rows_per_image: Some(extent.height),
                },
            },
            extent,
        );
        buffer
    }

    /// Waits for a readback and strips the row padding back out.
    fn take(&self, buffer: wgpu::Buffer, height: u32, layout: ReadbackLayout) -> Result<Vec<u8>> {
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| CadError::rendering_because("waiting for the frame", error))?;
        receiver
            .recv()
            .map_err(|error| CadError::rendering_because("the frame was never read back", error))?
            .map_err(|error| CadError::rendering_because("reading the frame back", error))?;

        let mapped = slice
            .get_mapped_range()
            .map_err(|error| CadError::rendering_because("mapping the frame", error))?;
        let mut out = Vec::new();
        out.try_reserve_exact(layout.tight_size)
            .map_err(|error| CadError::rendering_because("allocating the frame readback", error))?;
        for line in 0..height as usize {
            let at = line * layout.padded as usize;
            out.extend_from_slice(&mapped[at..at + layout.row]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy)]
struct ReadbackLayout {
    row: usize,
    padded: u32,
    buffer_size: u64,
    tight_size: usize,
}

/// One definition's geometry, resident on a device.
#[derive(Debug)]
struct GpuMesh {
    vertices: wgpu::Buffer,
    /// One face identity per vertex, parallel to `vertices`.
    faces: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    /// Where each face of this mesh stops, as pairs into the same vertices.
    ///
    /// Uploaded once beside the triangles and never rebuilt: a boundary is a
    /// fact about the tessellation, and neither the camera, the pointer, the
    /// selection nor what is hidden can change one.
    lines: wgpu::Buffer,
    line_index_count: u32,
    /// Two vertices per drawn edge segment, each carrying the identity this
    /// picture gave the topological edge it belongs to.
    ///
    /// Expanded rather than indexed, and that is the whole point. One position
    /// can be an end of several topological edges — a corner of a box is an end
    /// of three — so an identity stored once per position could not say which
    /// of them a segment belongs to. Uploaded once beside the triangles; no
    /// camera, pointer or selection changes it.
    edges: wgpu::Buffer,
    edge_vertex_count: u32,
}

/// A snapshot whose geometry is on a device and stays there.
///
/// Holds the [`RenderSnapshot`] it was built from as well as the buffers, so
/// what is on the device and what it means cannot be separated – the same
/// arrangement [`Frame`] uses, one step earlier.
///
/// Belongs to the renderer that prepared it. Another renderer refuses it rather
/// than drawing another device's memory.
#[derive(Debug)]
pub struct PreparedSnapshot {
    renderer: RendererId,
    snapshot: Arc<RenderSnapshot>,
    meshes: Vec<GpuMesh>,
    /// The per-draw uniforms are not held separately: a bind group keeps the
    /// resources it names alive, so a second handle to that buffer would only
    /// be a second thing to keep in step.
    bindings: wgpu::BindGroup,
}

impl PreparedSnapshot {
    /// What this was prepared from.
    pub fn snapshot(&self) -> &Arc<RenderSnapshot> {
        &self.snapshot
    }

    /// Which renderer owns these buffers.
    pub fn renderer(&self) -> RendererId {
        self.renderer
    }
}

/// One definition of the pipeline, whatever it is drawn into.
///
/// The colour format is the window's when there is a window and this crate's
/// own when there is not, and `with_identities` says whether the pick target is
/// there to be written. A second copy of this for the window path would be a
/// second place for the vertex layout, the depth state and the culling rule to
/// drift from each other.
/// The grid's pipeline, for whichever target it is drawn into.
///
/// One function for both, exactly as the model has one: the offscreen path
/// writes identities beside the colour and the window does not, and that is
/// the only difference there is allowed to be between the two grids.
///
/// Depth is tested and not written. The grid is a backdrop, so anything drawn
/// afterwards covers it wherever it appears, and a part below the plane is
/// still a part rather than something the floor hides.
fn build_grid_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    pipeline_layout: &wgpu::PipelineLayout,
    colour_format: wgpu::TextureFormat,
    with_identities: bool,
) -> wgpu::RenderPipeline {
    let mut targets = vec![Some(wgpu::ColorTargetState {
        format: colour_format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })];
    if with_identities {
        targets.push(Some(wgpu::ColorTargetState {
            format: PICK_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        }));
        targets.push(Some(wgpu::ColorTargetState {
            format: FACE_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        }));
    }

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ferritecad grid pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            // No buffers at all: every position comes from the vertex number,
            // so a zoom changes a uniform rather than uploading geometry.
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(identities_entry_point(with_identities)),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// Which fragment entry point writes exactly the given targets.
///
/// A shader whose output signature is wider than its pipeline is not a
/// harmless waste: Direct3D refuses to compile it. So the window and the
/// readback have an entry point each, over one shading function.
fn identities_entry_point(with_identities: bool) -> &'static str {
    if with_identities {
        "fragment_main"
    } else {
        "fragment_colour"
    }
}

/// Turns watched pipeline failures into the class callers can act on.
///
/// Validation means the renderer asked wgpu to build an invalid pipeline. It
/// is a rendering defect and must never look like a missing adapter to a pixel
/// test that is allowed to skip. An internal failure comes from the driver
/// after adapter discovery and remains an unsupported device. Validation wins
/// if a batch of pipeline builds reported both: hiding our own invalid request
/// behind a simultaneous driver problem would make the gate green by skipping.
fn pipeline_refusal(validation: Option<String>, internal: Option<String>) -> Result<()> {
    if let Some(refusal) = validation {
        return Err(CadError::rendering(format!(
            "the graphics pipeline this crate asked for is invalid: {refusal}"
        )));
    }
    if let Some(refusal) = internal {
        return Err(CadError::unsupported(format!(
            "this graphics adapter failed internally while building the pipeline: {refusal}"
        )));
    }
    Ok(())
}

/// The pipeline that draws where each face of a model stops.
///
/// The same shader, the same layout and the same vertices as the fill: only
/// the primitive is different, and the indices it is given. Nothing about the
/// camera is recomputed here, and nothing about identity is written: the pick
/// and face targets keep whatever the fill beneath the line put there, so a
/// line is drawn over a pixel without changing what that pixel is.
fn build_line_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    pipeline_layout: &wgpu::PipelineLayout,
    colour_format: wgpu::TextureFormat,
    with_identities: bool,
) -> wgpu::RenderPipeline {
    let mut targets = vec![Some(wgpu::ColorTargetState {
        format: colour_format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })];
    if with_identities {
        for format in [PICK_FORMAT, FACE_FORMAT] {
            targets.push(Some(wgpu::ColorTargetState {
                format,
                blend: None,
                // Written by nothing. A line is a thing to look at, not a
                // thing to click, and the picture must still answer for the
                // face underneath it.
                write_mask: wgpu::ColorWrites::empty(),
            }));
        }
    }

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ferritecad viewport line pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            // The model's own vertex stage. A line drawn through different
            // arithmetic from the surface it belongs to would part company
            // with it as soon as the camera moved.
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            buffers: &vertex_buffer_layouts(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(if with_identities {
                "fragment_line"
            } else {
                "fragment_line_colour"
            }),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            // Lines lie exactly on the surface they bound, so they must not
            // lose to it, and they leave the depth buffer as the fill left it.
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            // No bias: wgpu rejects one on a line topology outright. A line
            // and the triangle edge it came from are drawn from the same two
            // vertices through the same matrix, so the comparison above is
            // what lets the line win its own ties.
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// One definition's topological edges, as the vertices a line list draws.
///
/// Two vertices per segment, both carrying the identity this picture gave the
/// edge that owns the segment. Positions are read out of the packed mesh by
/// the very indices the kernel supplied, so an end that several edges meet at
/// is written once per edge rather than shared and guessed at afterwards.
///
/// A definition whose mesh carried no edge association, and one whose
/// association is empty, both produce nothing here: neither has an edge this
/// picture can name, so neither gets geometry.
fn edge_stream(
    snapshot: &RenderSnapshot,
    definition: usize,
    mesh: &ferritecad_viewport::PackedMesh,
) -> Result<Vec<EdgeVertex>> {
    let Some(count) = mesh.edge_count() else {
        return Ok(Vec::new());
    };
    let mut stream: Vec<EdgeVertex> = Vec::new();
    for ordinal in 0..count {
        // This picture's own numbering, asked for rather than recomputed: a
        // second account of it here is a second thing to drift.
        let edge = snapshot.edge_of(definition, ordinal).ok_or_else(|| {
            CadError::rendering(format!(
                "the picture numbers {count} edges of definition {definition} but \
                 will not name edge {ordinal}"
            ))
        })?;
        let raw = edge.to_raw();
        let segments = snapshot
            .segments_of_edge(edge)
            .ok_or_else(|| CadError::rendering("an edge of this picture draws nothing"))?;
        for pair in segments.chunks_exact(2) {
            for index in pair {
                let at = *index as usize * VERTEX_FLOATS;
                let position = mesh.vertices().get(at..at + 3).ok_or_else(|| {
                    CadError::rendering(format!(
                        "an edge segment names vertex {index}, which this mesh does not have"
                    ))
                })?;
                stream.push(EdgeVertex {
                    position: [position[0], position[1], position[2]],
                    edge: raw,
                });
            }
        }
    }
    Ok(stream)
}

/// The pipeline that marks the one topological edge under the pointer.
///
/// Colour and nothing else, whatever it is drawn into. That is what lets one
/// builder serve the window and the readback alike: both have a colour target
/// and the mark writes no identity, so there is one statement of what marking
/// an edge means rather than an offscreen one and a window one that could
/// drift.
///
/// Depth is tested and not written, exactly as the face boundaries are: the
/// mark lies on the surface it bounds and must not lose every tie to it, and
/// it must not answer through a nearer part. No bias, which wgpu refuses on a
/// line topology and which nothing here needs.
fn build_edge_mark_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    pipeline_layout: &wgpu::PipelineLayout,
    colour_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ferritecad viewport edge mark pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex_edge_mark"),
            compilation_options: Default::default(),
            buffers: &edge_vertex_buffer_layout(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fragment_edge_mark"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: colour_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// What a vertex of the expanded edge stream is, for either pipeline that
/// draws it. One layout, so the identity pass and the mark cannot disagree
/// about the buffer they share.
fn edge_vertex_buffer_layout() -> [Option<wgpu::VertexBufferLayout<'static>>; 1] {
    [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<EdgeVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: (3 * std::mem::size_of::<f32>()) as u64,
                shader_location: 1,
            },
        ],
    })]
}

/// The pipeline that says which topological edge a pixel is on.
///
/// Offscreen only, and one target: no colour, no definition and no face. The
/// pass it runs in has no such attachments, so this cannot disturb them by
/// accident rather than by policy.
///
/// Depth is tested against the model already drawn and never written. Tested,
/// so an edge behind a nearer part does not answer through it; `LessEqual`,
/// because an edge lies exactly on the surface it bounds and would otherwise
/// lose every tie to it; and not written, because this pass is about
/// identity and must leave the picture's depth exactly as the model left it.
/// No bias either way: wgpu rejects one outright on a line topology, and the
/// edge and the surface are projected from the same matrices, so there is
/// nothing to correct for.
fn build_edge_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    pipeline_layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ferritecad viewport edge identity pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex_edge"),
            compilation_options: Default::default(),
            buffers: &edge_vertex_buffer_layout(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fragment_edge"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: EDGE_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// What a vertex of the model is, for whichever pipeline draws it.
fn vertex_buffer_layouts() -> [Option<wgpu::VertexBufferLayout<'static>>; 2] {
    [
        Some(wgpu::VertexBufferLayout {
            array_stride: (VERTEX_FLOATS * std::mem::size_of::<f32>()) as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: (3 * std::mem::size_of::<f32>()) as u64,
                    shader_location: 1,
                },
            ],
        }),
        // The face identities, in their own buffer. A vertex belongs to
        // exactly one face, which is checked when the picture is packed, so
        // this says which face a fragment came from without asking the adapter
        // for a capability.
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<u32>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 0,
                shader_location: 2,
            }],
        }),
    ]
}

fn build_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    pipeline_layout: &wgpu::PipelineLayout,
    colour_format: wgpu::TextureFormat,
    with_identities: bool,
) -> wgpu::RenderPipeline {
    // The number of entries is part of the render-pass contract. `[colour,
    // None]` does not mean "one target" to wgpu: it means two attachment
    // slots, the second deliberately unwritten, and is incompatible with the
    // single attachment a window supplies. Build a genuinely shorter list for
    // that path.
    let mut targets = vec![Some(wgpu::ColorTargetState {
        format: colour_format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })];
    if with_identities {
        targets.push(Some(wgpu::ColorTargetState {
            format: PICK_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        }));
        // Faces travel with the identities and only there. A window draws its
        // model many times a second and asks what a pixel is when somebody
        // points at one, so paying for either on every frame would be paying
        // continuously for something wanted occasionally.
        targets.push(Some(wgpu::ColorTargetState {
            format: FACE_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        }));
    }

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ferritecad viewport pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            buffers: &vertex_buffer_layouts(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            // The entry point that writes exactly the targets this pipeline
            // has. Both shade the model identically; they differ only in what
            // they record about each pixel.
            entry_point: Some(identities_entry_point(with_identities)),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        primitive: wgpu::PrimitiveState {
            // No culling. An imported assembly is not obliged to have
            // consistent winding, and a part that vanished because of it
            // would be blamed on the import.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// One drawn frame, and the snapshot it was drawn from.
#[derive(Debug)]
pub struct Frame {
    snapshot: Arc<RenderSnapshot>,
    width: u32,
    height: u32,
    colour: Vec<u8>,
    picks: Vec<u32>,
    faces: Vec<u32>,
    edges: Vec<u32>,
}

/// What one pixel turned out to be: a definition, its face and any topological
/// edge rasterised over it.
///
/// All three together, because they were read from the same pixel of the same
/// frame and are only true of each other there. None is readable as a number
/// and none is stored anywhere. The constructor below refuses a face or edge
/// whose owner disagrees with the definition at that sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    definition: PickId,
    face: FacePickId,
    edge: EdgePickId,
}

impl Hit {
    /// Nothing was drawn here.
    pub const NOTHING: Self = Self {
        definition: PickId::NOTHING,
        face: FacePickId::NOTHING,
        edge: EdgePickId::NOTHING,
    };

    /// Which definition, exactly as [`Frame::pick_at`] would answer.
    pub fn definition(self) -> PickId {
        self.definition
    }

    /// Which face of it, for as long as this picture is on screen.
    pub fn face(self) -> FacePickId {
        self.face
    }

    /// Which topological edge of it lies under this pixel, if one does.
    pub fn edge(self) -> EdgePickId {
        self.edge
    }
}

impl Frame {
    /// The snapshot this frame shows.
    ///
    /// Held rather than referenced by identifier: a pick read against a
    /// different snapshot would resolve to whichever definition now occupies
    /// the number, and the two are kept together so that cannot be arranged.
    pub fn snapshot(&self) -> &RenderSnapshot {
        &self.snapshot
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Linear RGBA, four bytes per pixel, top row first.
    pub fn colour(&self) -> &[u8] {
        &self.colour
    }

    /// The colour at one pixel, or `None` outside the frame.
    pub fn colour_at(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        let at = self.index(x, y)? * 4;
        self.colour
            .get(at..at + 4)
            .map(|bytes| [bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// What was drawn at one pixel, resolved against this frame's own snapshot.
    ///
    /// Outside the frame, and anywhere nothing was drawn, this is
    /// [`PickId::NOTHING`].
    pub fn pick_at(&self, x: u32, y: u32) -> PickId {
        let Some(at) = self.index(x, y) else {
            return PickId::NOTHING;
        };
        match self.picks.get(at) {
            Some(raw) => PickId::from_raw(*raw, &self.snapshot),
            None => PickId::NOTHING,
        }
    }

    /// What was drawn at one pixel, which face of it and which topological edge
    /// was rasterised over that surface.
    ///
    /// Beside [`Self::pick_at`] rather than instead of it: what a click means
    /// is a settled question, and a hover asks a different one. Outside the
    /// frame, over the grid and over the background this is
    /// [`Hit::NOTHING`].
    pub fn hit_at(&self, x: u32, y: u32) -> Hit {
        let Some(at) = self.index(x, y) else {
            return Hit::NOTHING;
        };
        let definition = self.picks.get(at).map_or(PickId::NOTHING, |raw| {
            PickId::from_raw(*raw, &self.snapshot)
        });
        let candidate = self.faces.get(at).map_or(FacePickId::NOTHING, |raw| {
            FacePickId::from_raw(*raw, &self.snapshot)
        });
        // Both targets describe one fragment. If a readback ever contradicts
        // itself, preserve the established definition-pick semantics and say
        // no face rather than manufacture a face of a different definition.
        let face = if self.snapshot.definition(definition)
            == self.snapshot.definition_of_face(candidate)
        {
            candidate
        } else {
            FacePickId::NOTHING
        };
        // The same rule again, for the edge. An edge is drawn over the surface
        // it bounds, so the two answers are about one pixel and must agree
        // about whose pixel it is. Where they do not — the outer silhouette,
        // where a line lands on a pixel the fill did not reach — this says no
        // edge rather than an edge of some other definition, and leaves the
        // definition and the face exactly as they were.
        let candidate = self.edges.get(at).map_or(EdgePickId::NOTHING, |raw| {
            EdgePickId::from_raw(*raw, &self.snapshot)
        });
        let edge = if self.snapshot.definition(definition)
            == self.snapshot.definition_of_edge(candidate)
        {
            candidate
        } else {
            EdgePickId::NOTHING
        };
        Hit {
            definition,
            face,
            edge,
        }
    }

    /// Which topological edge of the model was drawn at one pixel.
    ///
    /// Resolved against this frame's own snapshot, exactly as a pick is.
    /// Outside the frame, over the cleared grid or backdrop and anywhere inside
    /// a face this is [`EdgePickId::NOTHING`]. At an outer silhouette the line
    /// rasteriser can cover a neighbouring sample the filled triangle did not;
    /// this raw target still reports the edge there, while [`Self::hit_at`]
    /// refuses it because that sample has no agreeing definition.
    ///
    /// The raw answer for the pixel and nothing more. [`Self::hit_at`] is
    /// where an edge is required to agree with the definition under it; this
    /// says what the target holds. A single integer target can retain only one
    /// answer where several edges cover the same sample, at a shared endpoint
    /// or projected crossing; the deterministic draw order decides which one.
    pub fn edge_at(&self, x: u32, y: u32) -> EdgePickId {
        let Some(at) = self.index(x, y) else {
            return EdgePickId::NOTHING;
        };
        match self.edges.get(at) {
            Some(raw) => EdgePickId::from_raw(*raw, &self.snapshot),
            None => EdgePickId::NOTHING,
        }
    }

    fn index(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height).then(|| (y * self.width + x) as usize)
    }
}

/// Little-endian `u32` per pixel, as an `R32Uint` target lands.
fn unpack(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .collect()
}

fn align_to(value: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        return value;
    }
    value.div_ceil(alignment) * alignment
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use ferritecad_kernel::{
        Mesh, MeshFaceRange, SessionId, ShapeHandle, SubShapeHandle, SubShapeKind,
    };
    use ferritecad_types::ErrorKind;
    use ferritecad_types::Transform;
    use ferritecad_viewport::SnapshotBuilder;

    #[test]
    fn an_invalid_pipeline_is_a_rendering_failure_and_never_a_skippable_adapter() {
        let validation = pipeline_refusal(Some("bad line state".to_owned()), None)
            .expect_err("an invalid pipeline was accepted");
        assert_eq!(validation.kind(), ErrorKind::Rendering, "{validation}");
        assert!(
            validation.to_string().contains("bad line state"),
            "the driver words were lost: {validation}"
        );

        let internal = pipeline_refusal(None, Some("driver stopped".to_owned()))
            .expect_err("an internal driver refusal was accepted");
        assert_eq!(internal.kind(), ErrorKind::Unsupported, "{internal}");

        let both = pipeline_refusal(
            Some("invalid request".to_owned()),
            Some("driver stopped too".to_owned()),
        )
        .expect_err("two failures were accepted");
        assert_eq!(
            both.kind(),
            ErrorKind::Rendering,
            "validation did not take priority: {both}"
        );
        pipeline_refusal(None, None).expect("no pipeline failure was reported");
    }

    fn one_quad(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
        let shape = ShapeHandle::new(SessionId::new(), 1);
        let mesh = Mesh {
            // XZ, facing the camera that `frame` places along -Y.
            positions: vec![
                -10.0, 0.0, -10.0, 10.0, 0.0, -10.0, 10.0, 0.0, 10.0, -10.0, 0.0, 10.0,
            ],
            normals: vec![
                0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            faces: vec![MeshFaceRange {
                face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 6,
            }],
            edges: None,
        };
        let mut builder = SnapshotBuilder::new();
        let definition = builder.add_mesh(&mesh).expect("packs the quad");
        builder
            .place(definition, None, &Transform::IDENTITY, [0.0, 1.0, 0.0])
            .expect("places the quad");
        let snapshot = Arc::new(builder.build());
        let mut camera = Camera::new();
        camera.resize(width, height);
        camera
            .frame(snapshot.bounds().expect("the quad has an extent"))
            .expect("frames the quad");
        (snapshot, camera)
    }

    #[test]
    fn the_window_pipeline_really_has_one_colour_target() {
        let mut renderer = match Renderer::new() {
            Ok(renderer) => renderer,
            Err(error) if error.kind() == ErrorKind::Unsupported => {
                eprintln!("skipped: {error}");
                return;
            }
            Err(error) => panic!("a renderer failed after adapter discovery: {error}"),
        };

        // No surface is needed to state the contract that failed in the real
        // window: a surface contributes one colour view, and the pipeline used
        // with it must accept a pass with exactly that one attachment. Draw a
        // real quad and read it back so merely accepting an empty pass cannot
        // satisfy the gate.
        let width = 64;
        let height = 64;
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ferritecad single-target window gate"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let (snapshot, camera) = one_quad(width, height);
        let prepared = renderer
            .prepare(snapshot)
            .expect("uploads the window scene");

        renderer
            .draw_into(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &Visibility::default(),
                &view,
                format,
                width,
                height,
            )
            .expect("one-target pass accepts the one-target pipeline");

        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let layout = renderer
            .validate_frame(width, height)
            .expect("the frame fits");
        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ferritecad single-target window readback"),
            });
        let readback = renderer.readback(&mut encoder, &target, extent, layout);
        renderer.queue.submit(Some(encoder.finish()));
        let pixels = renderer
            .take(readback, height, layout)
            .expect("reads the window target");
        let centre = ((height / 2 * width + width / 2) * 4) as usize;
        assert!(
            pixels[centre + 1] > 0,
            "the one-target pass accepted its pipeline but drew no green quad"
        );
    }

    /// Draws one frame through the window path and reads the colour back.
    ///
    /// The window path presents rather than reads back, so this is the only
    /// way to compare what it draws with what the readback path draws. It is
    /// exactly `draw_into` followed by a copy: nothing about which definitions
    /// are drawn is decided here.
    fn window_colour(
        renderer: &mut Renderer,
        prepared: &PreparedSnapshot,
        camera: &Camera,
        visibility: &Visibility,
    ) -> Vec<u8> {
        window_colour_marked(
            renderer,
            prepared,
            camera,
            Marked::Nothing,
            Hovered::Nothing,
            visibility,
        )
    }

    /// The same window path, told what is chosen and what is asked about.
    fn window_colour_marked(
        renderer: &mut Renderer,
        prepared: &PreparedSnapshot,
        camera: &Camera,
        selected: Marked,
        hovered: Hovered,
        visibility: &Visibility,
    ) -> Vec<u8> {
        let (width, height) = (camera.width(), camera.height());
        let format = COLOUR_FORMAT;
        let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ferritecad window comparison"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        renderer
            .draw_into(
                prepared, camera, selected, hovered, visibility, &view, format, width, height,
            )
            .expect("the window path draws");

        let layout = renderer
            .validate_frame(width, height)
            .expect("the frame fits");
        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ferritecad window comparison readback"),
            });
        let readback = renderer.readback(
            &mut encoder,
            &target,
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            layout,
        );
        renderer.queue.submit(Some(encoder.finish()));
        renderer
            .take(readback, height, layout)
            .expect("reads the window target")
    }

    #[test]
    fn the_window_pipeline_hides_exactly_what_the_readback_pipeline_hides() {
        let mut renderer = match Renderer::new() {
            Ok(renderer) => renderer,
            Err(error) if error.kind() == ErrorKind::Unsupported => {
                eprintln!("skipped: {error}");
                return;
            }
            Err(error) => panic!("a renderer failed after adapter discovery: {error}"),
        };

        // Two quads, one behind the other, so hiding one changes the picture.
        let shape = |id| ShapeHandle::new(SessionId::new(), id);
        let plate = |half: f32, y: f32, id: u64| Mesh {
            positions: vec![
                -half, y, -half, half, y, -half, half, y, half, -half, y, half,
            ],
            normals: vec![
                0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            faces: vec![MeshFaceRange {
                face: SubShapeHandle::new(shape(id), SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 6,
            }],
            edges: None,
        };
        let mut builder = SnapshotBuilder::new();
        let front = builder.add_mesh(&plate(20.0, 0.0, 1)).expect("packs");
        let rear = builder.add_mesh(&plate(4.0, 9.0, 2)).expect("packs");
        builder
            .place(front, None, &Transform::IDENTITY, [0.8, 0.2, 0.2])
            .expect("places");
        builder
            .place(rear, None, &Transform::IDENTITY, [0.2, 0.4, 0.9])
            .expect("places");
        let snapshot = Arc::new(builder.build());
        let mut camera = Camera::new();
        camera.resize(64, 64);
        camera
            .frame(snapshot.bounds().expect("an extent"))
            .expect("frames");
        let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");

        let everything = Visibility::new(&snapshot);
        let mut hiding = everything.clone();
        assert!(hiding.hide(
            Marked::Definition(snapshot.pick_of(0).expect("drawn")),
            &snapshot
        ));

        // Both paths, both states. A filter applied in one and not the other
        // would show a part in the window that a click cannot reach, or the
        // reverse.
        let offscreen_all = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &everything,
            )
            .expect("draws")
            .colour()
            .to_vec();
        let offscreen_hiding = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &hiding,
            )
            .expect("draws")
            .colour()
            .to_vec();
        assert_ne!(
            offscreen_all, offscreen_hiding,
            "the gate compared two identical pictures"
        );

        // The picture being compared contains linework, so the comparisons
        // below are comparisons of a picture with lines in it rather than of
        // one that happens to have none.
        let ink = offscreen_all
            .chunks_exact(4)
            .filter(|pixel| {
                let luminance = 0.2126 * f32::from(pixel[0])
                    + 0.7152 * f32::from(pixel[1])
                    + 0.0722 * f32::from(pixel[2]);
                luminance < 30.0 && pixel[3] > 0
            })
            .count();
        assert!(
            ink > 20,
            "the gate compared a picture with no linework: {ink}"
        );

        assert_eq!(
            window_colour(&mut renderer, &prepared, &camera, &everything),
            offscreen_all,
            "the window and the readback disagree about what is drawn"
        );
        assert_eq!(
            window_colour(&mut renderer, &prepared, &camera, &hiding),
            offscreen_hiding,
            "the window and the readback disagree about what is hidden"
        );

        // And the same through the other projection, which is one matrix and
        // not a second path: what a window draws and what a click reads must
        // agree about how the world reaches the screen.
        let mut flat = camera;
        assert!(flat.set_projection(ferritecad_viewport::Projection::Orthographic));
        let offscreen_flat = renderer
            .render(
                &prepared,
                &flat,
                Marked::Nothing,
                Hovered::Nothing,
                &everything,
            )
            .expect("draws")
            .colour()
            .to_vec();
        assert_ne!(
            offscreen_flat, offscreen_all,
            "the projection changed nothing"
        );
        assert_eq!(
            window_colour(&mut renderer, &prepared, &flat, &everything),
            offscreen_flat,
            "the window and the readback disagree about the projection"
        );

        // And the same after a wheel aimed away from the middle, in both
        // projections. An anchored zoom is camera state and nothing else, so a
        // window that read it differently from a readback would put the pointer
        // over one part and a click on another.
        for mut aimed in [camera, flat] {
            let projection = aimed.projection_mode();
            aimed.zoom_at(0.45, 22.0, -17.0);
            let offscreen_aimed = renderer
                .render(
                    &prepared,
                    &aimed,
                    Marked::Nothing,
                    Hovered::Nothing,
                    &everything,
                )
                .expect("draws")
                .colour()
                .to_vec();
            assert_ne!(
                offscreen_aimed, offscreen_all,
                "{projection:?}: the wheel changed nothing"
            );
            assert_eq!(
                window_colour(&mut renderer, &prepared, &aimed, &everything),
                offscreen_aimed,
                "{projection:?}: the window and the readback disagree about an aimed zoom"
            );
        }

        // And the same with the horizon turned, in both projections. A roll
        // is camera state like any other, so a window that read it differently
        // from a readback would draw the model at one angle and answer clicks
        // at another.
        for mut turned in [camera, flat] {
            let projection = turned.projection_mode();
            turned.roll(0.6);
            let offscreen_turned = renderer
                .render(
                    &prepared,
                    &turned,
                    Marked::Nothing,
                    Hovered::Nothing,
                    &everything,
                )
                .expect("draws")
                .colour()
                .to_vec();
            assert_ne!(
                offscreen_turned, offscreen_all,
                "{projection:?}: the turn changed nothing"
            );
            assert_eq!(
                window_colour(&mut renderer, &prepared, &turned, &everything),
                offscreen_turned,
                "{projection:?}: the window and the readback disagree about a turned view"
            );
        }

        // And the same for a camera that a smart magnification framed tight
        // on part of the picture, and for the one it would go back to. A
        // camera is a camera whatever moved it, so this is the window and the
        // readback agreeing about a fit rather than about a gesture.
        for mut fitted in [camera, flat] {
            let projection = fitted.projection_mode();
            fitted
                .frame(([-4.0, 8.0, -4.0], [4.0, 10.0, 4.0]))
                .expect("a part of the picture is framable");
            let offscreen_fitted = renderer
                .render(
                    &prepared,
                    &fitted,
                    Marked::Nothing,
                    Hovered::Nothing,
                    &everything,
                )
                .expect("draws")
                .colour()
                .to_vec();
            assert_ne!(
                offscreen_fitted, offscreen_all,
                "{projection:?}: framing tight changed nothing"
            );
            assert_eq!(
                window_colour(&mut renderer, &prepared, &fitted, &everything),
                offscreen_fitted,
                "{projection:?}: the window and the readback disagree about a magnified view"
            );
        }

        // And the same for the mask isolating leaves behind, which is the same
        // mask reached a different way: one representation, one consumer.
        let mut isolating = everything.clone();
        assert!(isolating.isolate(
            Marked::Definition(snapshot.pick_of(1).expect("drawn")),
            &snapshot
        ));
        let offscreen_isolating = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &isolating,
            )
            .expect("draws")
            .colour()
            .to_vec();
        assert_ne!(offscreen_isolating, offscreen_all);
        assert_eq!(
            window_colour(&mut renderer, &prepared, &camera, &isolating),
            offscreen_isolating,
            "the window and the readback disagree about what is isolated"
        );

        // And once one definition has been asked back, which is a third mask
        // reached a third way through the one representation both paths read.
        let mut shown = isolating.clone();
        assert!(shown.show(
            Marked::Definition(snapshot.pick_of(0).expect("drawn")),
            &snapshot
        ));
        let offscreen_shown = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &shown,
            )
            .expect("draws")
            .colour()
            .to_vec();
        assert_ne!(offscreen_shown, offscreen_isolating);
        assert_eq!(
            window_colour(&mut renderer, &prepared, &camera, &shown),
            offscreen_shown,
            "the window and the readback disagree about what came back"
        );
    }

    #[test]
    fn what_a_frame_is_drawn_against_is_the_size_the_shader_declares() {
        // The assertion beside the type is the real gate; this states the
        // number in a place a reader looking for it will find, and fails
        // loudly rather than at whichever driver notices a short binding.
        assert_eq!(
            std::mem::size_of::<GlobalsUniform>(),
            96,
            "Globals must stay the size WGSL rounds it to"
        );
        assert_eq!(std::mem::align_of::<GlobalsUniform>(), 4);
    }

    #[test]
    fn a_window_marks_the_same_edge_the_readback_marks() {
        let mut renderer = match Renderer::new() {
            Ok(renderer) => renderer,
            Err(error) if error.kind() == ErrorKind::Unsupported => {
                eprintln!("skipped: {error}");
                return;
            }
            Err(error) => panic!("a renderer failed after adapter discovery: {error}"),
        };

        // Two faces of one definition meeting along an edge, so there is a
        // real edge to mark rather than an empty frame to compare.
        let shape = ShapeHandle::new(SessionId::new(), 9);
        let mesh = Mesh {
            positions: vec![
                -6.0, 0.0, -6.0, 6.0, 0.0, -6.0, 6.0, 0.0, 0.0, -6.0, 0.0, 0.0, //
                -6.0, 0.0, 0.0, 6.0, 0.0, 0.0, 6.0, 0.0, 6.0, -6.0, 0.0, 6.0,
            ],
            normals: [0.0, -1.0, 0.0].repeat(8),
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            faces: (0..2)
                .map(|face| MeshFaceRange {
                    face: SubShapeHandle::new(shape, SubShapeKind::Face, face),
                    first_index: face as u32 * 6,
                    index_count: 6,
                })
                .collect(),
            edges: Some(ferritecad_kernel::MeshEdges {
                segments: vec![3, 2, 4, 5, 0, 1],
                ranges: vec![
                    ferritecad_kernel::MeshEdgeRange {
                        edge: SubShapeHandle::new(shape, SubShapeKind::Edge, 0),
                        first_segment: 0,
                        segment_count: 2,
                    },
                    ferritecad_kernel::MeshEdgeRange {
                        edge: SubShapeHandle::new(shape, SubShapeKind::Edge, 1),
                        first_segment: 2,
                        segment_count: 1,
                    },
                ],
            }),
        };
        let mut builder = SnapshotBuilder::new();
        let definition = builder.add_mesh(&mesh).expect("packs");
        builder
            .place(definition, None, &Transform::IDENTITY, [0.15, 0.5, 0.85])
            .expect("places");
        let snapshot = Arc::new(builder.build());
        let mut camera = Camera::new();
        camera.resize(192, 192);
        camera
            .frame(snapshot.bounds().expect("drawn"))
            .expect("frames");
        let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
        let visibility = Visibility::new(&snapshot);
        let edge = snapshot.edge_of(0, 0).expect("numbered");

        let plain = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws")
            .colour()
            .to_vec();
        let marked = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Edge(edge),
                &visibility,
            )
            .expect("draws")
            .colour()
            .to_vec();
        // The comparison is worth making only if there is a mark in it.
        assert_ne!(plain, marked, "the readback drew no mark to compare");

        assert_eq!(
            window_colour_marked(
                &mut renderer,
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Edge(edge),
                &visibility,
            ),
            marked,
            "the window and the readback marked different pixels"
        );
        assert_eq!(
            window_colour_marked(
                &mut renderer,
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            ),
            plain,
            "the window and the readback disagree without a mark"
        );
    }

    #[test]
    fn contradictory_targets_never_make_a_face_of_another_definition() {
        let shape = ShapeHandle::new(SessionId::new(), 1);
        let mesh = |ordinal| Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: SubShapeHandle::new(shape, SubShapeKind::Face, ordinal),
                first_index: 0,
                index_count: 3,
            }],
            edges: Some(ferritecad_kernel::MeshEdges {
                segments: vec![0, 1],
                ranges: vec![ferritecad_kernel::MeshEdgeRange {
                    edge: SubShapeHandle::new(shape, SubShapeKind::Edge, ordinal),
                    first_segment: 0,
                    segment_count: 1,
                }],
            }),
        };
        let mut builder = SnapshotBuilder::new();
        builder.add_mesh(&mesh(0)).expect("packs first");
        builder.add_mesh(&mesh(1)).expect("packs second");
        let snapshot = Arc::new(builder.build());
        let frame = Frame {
            snapshot: Arc::clone(&snapshot),
            width: 1,
            height: 1,
            colour: vec![0; 4],
            picks: vec![1],
            // Face two belongs to definition two, not the definition target.
            faces: vec![2],
            // And so does edge two.
            edges: vec![2],
        };

        let hit = frame.hit_at(0, 0);
        assert_eq!(hit.definition(), snapshot.pick_of(0).expect("first pick"));
        assert_eq!(hit.face(), FacePickId::NOTHING);
        assert_eq!(
            hit.edge(),
            EdgePickId::NOTHING,
            "an edge of another definition was kept"
        );
        // The raw target is reported as it stands. Refusing it is what a hit
        // does about a contradiction, not what a readback does about a value.
        assert_eq!(
            frame.edge_at(0, 0),
            snapshot
                .edge_of(1, 0)
                .expect("the second definition's edge"),
        );

        // The agreeing case, so the refusal above is about the contradiction
        // and not about edges never surviving at all.
        let agreeing = Frame {
            snapshot: Arc::clone(&snapshot),
            width: 1,
            height: 1,
            colour: vec![0; 4],
            picks: vec![1],
            faces: vec![1],
            edges: vec![1],
        };
        let hit = agreeing.hit_at(0, 0);
        assert_eq!(hit.definition(), snapshot.pick_of(0).expect("first pick"));
        assert_eq!(hit.face(), snapshot.face_of(0, 0).expect("first face"));
        assert_eq!(hit.edge(), snapshot.edge_of(0, 0).expect("first edge"));
    }

    #[test]
    fn an_edge_value_from_outside_the_frame_or_the_picture_names_nothing() {
        let shape = ShapeHandle::new(SessionId::new(), 1);
        let mesh = Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 3,
            }],
            edges: Some(ferritecad_kernel::MeshEdges {
                segments: vec![0, 1],
                ranges: vec![ferritecad_kernel::MeshEdgeRange {
                    edge: SubShapeHandle::new(shape, SubShapeKind::Edge, 0),
                    first_segment: 0,
                    segment_count: 1,
                }],
            }),
        };
        let mut builder = SnapshotBuilder::new();
        builder.add_mesh(&mesh).expect("packs");
        let snapshot = Arc::new(builder.build());
        let frame = Frame {
            snapshot,
            width: 1,
            height: 1,
            colour: vec![0; 4],
            picks: vec![1],
            faces: vec![1],
            // A number no edge of this picture carries.
            edges: vec![9],
        };

        assert_eq!(frame.edge_at(0, 0), EdgePickId::NOTHING);
        assert_eq!(
            frame.edge_at(1, 0),
            EdgePickId::NOTHING,
            "outside the width"
        );
        assert_eq!(
            frame.edge_at(0, 1),
            EdgePickId::NOTHING,
            "outside the height"
        );
        assert_eq!(frame.hit_at(0, 0).edge(), EdgePickId::NOTHING);
    }
}
