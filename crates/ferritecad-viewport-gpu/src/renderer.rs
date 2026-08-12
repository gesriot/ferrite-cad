// SPDX-License-Identifier: MIT
//! The device, the targets and one pass over a snapshot.

use std::sync::Arc;

use ferritecad_types::{CadError, Result};
use ferritecad_viewport::{Camera, PickId, RenderSnapshot, VERTEX_FLOATS};
use wgpu::util::DeviceExt as _;

/// Linear, not sRGB. The snapshot's colours are linear because that is what the
/// importer read out of the file, and a target that encoded on write would make
/// the bytes read back a statement about a transfer function rather than about
/// what was drawn.
pub const COLOUR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// One unsigned integer per pixel: an identity is not a colour, and storing it
/// in one would mean packing it into channels and hoping nothing filters it.
pub const PICK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;

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
    padding: [u32; 3],
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
                    visibility: wgpu::ShaderStages::VERTEX,
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

        let pipeline = build_pipeline(&device, &shader, &pipeline_layout, COLOUR_FORMAT, true);

        let draw_stride = align_to(
            std::mem::size_of::<DrawUniform>() as u64,
            u64::from(device.limits().min_uniform_buffer_offset_alignment),
        );

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ferritecad viewport globals"),
            size: std::mem::size_of::<[f32; 16]>() as u64,
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
    pub(crate) fn draw_into(
        &mut self,
        prepared: &PreparedSnapshot,
        camera: &Camera,
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

        self.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::cast_slice(&camera.view_projection()),
        );

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
        if self.surface_pipelines.contains_key(&format) {
            return;
        }
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
    pub fn render(&mut self, prepared: &PreparedSnapshot, camera: &Camera) -> Result<Frame> {
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

        // The one thing that differs from frame to frame.
        self.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::cast_slice(&camera.view_projection()),
        );

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
        self.queue.submit(Some(encoder.finish()));

        let colour = self.take(colour_read, height, readback)?;
        let picks = self.take(pick_read, height, readback)?;

        Ok(Frame {
            snapshot,
            width,
            height,
            colour,
            picks: picks
                .chunks_exact(4)
                .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                .collect(),
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
fn build_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    pipeline_layout: &wgpu::PipelineLayout,
    colour_format: wgpu::TextureFormat,
    with_identities: bool,
) -> wgpu::RenderPipeline {
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
            entry_point: Some("fragment_main"),
            compilation_options: Default::default(),
            targets: &[
                Some(wgpu::ColorTargetState {
                    format: colour_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                // The fragment shader always writes an identity. A pass with
                // no target for it discards the value rather than needing a
                // second shader that does not compute it.
                with_identities.then_some(wgpu::ColorTargetState {
                    format: PICK_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
            ],
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

    fn index(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height).then(|| (y * self.width + x) as usize)
    }
}

fn align_to(value: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        return value;
    }
    value.div_ceil(alignment) * alignment
}
