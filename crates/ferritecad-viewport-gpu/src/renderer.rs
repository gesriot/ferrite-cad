// SPDX-License-Identifier: MIT
//! The device, the targets and one pass over a snapshot.

use std::sync::Arc;

use ferritecad_types::{CadError, Result};
use ferritecad_viewport::{Camera, FacePickId, Hovered, PickId, RenderSnapshot, VERTEX_FLOATS};
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

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

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
    /// Where this draw's mesh begins in the picture's table of face
    /// identities. A triangle knows its own number within the draw; this is
    /// what turns that into a face of the whole picture.
    first_triangle: u32,
    padding: [u32; 2],
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
    /// The face the pointer is over, or zero. A face of the picture, so the
    /// same face is marked in every placement of its definition.
    hovered_face: u32,
    /// The identity the pointer is over, or zero. Kept apart from the
    /// selection because they are different states and a person must be able
    /// to tell which is which: one is a decision and the other is a question.
    hovered: u32,
    padding: [u32; 1],
}

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
    /// One pipeline per surface format met so far. A window chooses its own
    /// format and a pipeline is built for the format it is drawn into, not for
    /// the one this crate would have preferred.
    surface_pipelines: std::collections::HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
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
        // Naming a face means knowing which triangle a pixel came from, and
        // that is a capability rather than a technique: without it the choice
        // is a draw call per face or a copy of every shared vertex. Refused
        // here, once and by name, rather than producing a viewer whose hover
        // silently marks whole parts on some machines.
        if !adapter.features().contains(wgpu::Features::PRIMITIVE_INDEX) {
            return Err(CadError::unsupported(
                "this graphics adapter cannot tell which triangle a pixel came from, which is what naming a face needs",
            ));
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ferritecad viewport"),
            required_features: wgpu::Features::PRIMITIVE_INDEX,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    // Read in the fragment stage alone: which face a triangle
                    // belongs to is a question about the pixel being drawn.
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
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
            surface_pipelines: std::collections::HashMap::new(),
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
        selected: PickId,
        hovered: Hovered,
    ) {
        // Every identity asked of the picture about to be drawn, and by the
        // same question. A number that named a definition of some other
        // picture would otherwise light up whichever one occupies it here.
        let snapshot = prepared.snapshot();
        let known = |pick: PickId| match snapshot.definition(pick) {
            Some(_) => pick.to_raw(),
            None => PickId::NOTHING.to_raw(),
        };
        let (hovered_face, hovered) = match hovered {
            Hovered::Nothing => (FacePickId::NOTHING.to_raw(), PickId::NOTHING.to_raw()),
            Hovered::Definition(pick) => (FacePickId::NOTHING.to_raw(), known(pick)),
            Hovered::Face(face) => match snapshot.definition_of_face(face) {
                Some(_) => (face.to_raw(), PickId::NOTHING.to_raw()),
                None => (FacePickId::NOTHING.to_raw(), PickId::NOTHING.to_raw()),
            },
        };
        self.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&GlobalsUniform {
                view_projection: camera.view_projection(),
                selected: known(selected),
                hovered_face,
                hovered,
                padding: [0; 1],
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
        selected: PickId,
        hovered: Hovered,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.require_own(prepared)?;

        self.ensure_surface_pipeline(format);
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
            for (index, item) in prepared.snapshot.draws().iter().enumerate() {
                let mesh = &prepared.meshes[item.mesh];
                if mesh.index_count == 0 {
                    continue;
                }
                pass.set_bind_group(
                    0,
                    &prepared.bindings,
                    &[(index as u64 * self.draw_stride) as u32],
                );
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
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
    pub(crate) fn present(&self, texture: wgpu::SurfaceTexture) {
        self.queue.present(texture);
    }

    /// Builds the pipeline for a window format, once per format met.
    fn ensure_surface_pipeline(&mut self, format: wgpu::TextureFormat) {
        if !self.surface_pipelines.contains_key(&format) {
            let pipeline = build_pipeline(
                &self.device,
                &self.shader,
                &self.pipeline_layout,
                format,
                // No identity target: see `draw_into`.
                false,
            );
            self.surface_pipelines.insert(format, pipeline);
        }
        if !self.grid_surface_pipelines.contains_key(&format) {
            let pipeline = build_grid_pipeline(
                &self.device,
                &self.grid_shader,
                &self.grid_pipeline_layout,
                format,
                false,
            );
            self.grid_surface_pipelines.insert(format, pipeline);
        }
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

        // The whole picture's faces in one table, in mesh order, uploaded once
        // with the geometry. A triangle's face is a lookup rather than a
        // vertex attribute, so nothing is duplicated and no draw is split.
        let mut face_table: Vec<u32> = Vec::new();
        let mut first_triangle_of = Vec::with_capacity(snapshot.meshes().len());
        for mesh in snapshot.meshes() {
            first_triangle_of.push(face_table.len() as u32);
            face_table.extend_from_slice(mesh.faces_of_triangles());
        }
        if face_table.is_empty() {
            // A device will not bind an empty storage buffer, and a picture
            // with no faces still has to draw.
            face_table.push(0);
        }
        let faces = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ferritecad viewport faces"),
                contents: bytemuck::cast_slice(&face_table),
                // Storage and nothing else: no COPY_DST, so this cannot be
                // rewritten later. What a face is was decided when the picture
                // was packed.
                usage: wgpu::BufferUsages::STORAGE,
            });
        self.geometry_uploads += 1;

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
                first_triangle: first_triangle_of[item.mesh],
                padding: [0; 2],
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: faces.as_entire_binding(),
                },
            ],
        });

        // One vertex and one index buffer per packed mesh.
        let mut meshes = Vec::new();
        meshes
            .try_reserve_exact(snapshot.meshes().len())
            .map_err(|error| CadError::rendering_because("recording mesh buffers", error))?;
        for mesh in snapshot.meshes() {
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
            meshes.push(GpuMesh {
                vertices,
                indices,
                index_count: mesh.indices().len() as u32,
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
        selected: PickId,
        hovered: Hovered,
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

        let bindings = &prepared.bindings;
        let geometry = &prepared.meshes;

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
                        store: wgpu::StoreOp::Discard,
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
            // In the order the snapshot lists them. A renderer that sorted to
            // save state changes would make two frames of one model differ.
            for (index, item) in snapshot.draws().iter().enumerate() {
                let mesh = &geometry[item.mesh];
                if mesh.index_count == 0 {
                    continue;
                }
                pass.set_bind_group(0, bindings, &[(index as u64 * self.draw_stride) as u32]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }

        let colour_read = self.readback(&mut encoder, &colour, extent, readback);
        let pick_read = self.readback(&mut encoder, &pick, extent, readback);
        let face_read = self.readback(&mut encoder, &face, extent, readback);
        self.queue.submit(Some(encoder.finish()));

        let colour = self.take(colour_read, height, readback)?;
        let picks = self.take(pick_read, height, readback)?;
        let faces = self.take(face_read, height, readback)?;

        Ok(Frame {
            snapshot,
            width,
            height,
            colour,
            picks: unpack(&picks),
            faces: unpack(&faces),
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
    indices: wgpu::Buffer,
    index_count: u32,
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
            buffers: &[Some(wgpu::VertexBufferLayout {
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
            })],
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
}

/// What one pixel turned out to be: a definition, and the face of it.
///
/// Both, together, because they were read from the same pixel of the same
/// frame and are only true of each other there. Neither field is readable as a
/// number and neither is stored anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    definition: PickId,
    face: FacePickId,
}

impl Hit {
    /// Nothing was drawn here.
    pub const NOTHING: Self = Self {
        definition: PickId::NOTHING,
        face: FacePickId::NOTHING,
    };

    /// Which definition, exactly as [`Frame::pick_at`] would answer.
    pub fn definition(self) -> PickId {
        self.definition
    }

    /// Which face of it, for as long as this picture is on screen.
    pub fn face(self) -> FacePickId {
        self.face
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

    /// What was drawn at one pixel, and which face of it.
    ///
    /// Beside [`Self::pick_at`] rather than instead of it: what a click means
    /// is a settled question, and a hover asks a different one. Outside the
    /// frame, over the grid and over the background this is
    /// [`Hit::NOTHING`].
    pub fn hit_at(&self, x: u32, y: u32) -> Hit {
        let Some(at) = self.index(x, y) else {
            return Hit::NOTHING;
        };
        Hit {
            definition: match self.picks.get(at) {
                Some(raw) => PickId::from_raw(*raw, &self.snapshot),
                None => PickId::NOTHING,
            },
            face: match self.faces.get(at) {
                Some(raw) => FacePickId::from_raw(*raw, &self.snapshot),
                None => FacePickId::NOTHING,
            },
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
    #[ignore = "probe branch only: this is the failing gate, and cargo stops at the first failing binary"]
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
                PickId::NOTHING,
                Hovered::Nothing,
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
}
