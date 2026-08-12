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

/// A device, a pipeline and the textures one frame is drawn into.
#[derive(Debug)]
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// The alignment a dynamic uniform offset must be a multiple of, which is a
    /// property of the device and not a constant anyone may assume.
    draw_stride: u64,
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

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ferritecad viewport"),
            ..Default::default()
        }))
        .map_err(|error| {
            CadError::unsupported(format!(
                "a graphics adapter was found but refused a device: {error}"
            ))
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

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ferritecad viewport pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
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
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: COLOUR_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
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
        });

        let draw_stride = align_to(
            std::mem::size_of::<DrawUniform>() as u64,
            u64::from(device.limits().min_uniform_buffer_offset_alignment),
        );

        Ok(Self {
            device,
            queue,
            pipeline,
            layout,
            draw_stride,
        })
    }

    /// Draws one snapshot through one camera and reads the result back.
    ///
    /// The snapshot is moved into the returned frame, so what was drawn and
    /// what a pick is interpreted against cannot come apart.
    ///
    /// Buffers are uploaded per call. There is no window yet and so nothing
    /// draws continuously; keeping geometry resident belongs with the slice
    /// that has something to keep it resident *for*.
    pub fn render(&mut self, snapshot: Arc<RenderSnapshot>, camera: &Camera) -> Result<Frame> {
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

        let globals = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ferritecad viewport globals"),
                contents: bytemuck::cast_slice(&camera.view_projection()),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        // Every draw's uniform data in one buffer, each at a device-aligned
        // offset. An empty snapshot still needs one stride of buffer, because a
        // zero-sized uniform buffer is not a thing a device will bind.
        let mut draw_bytes =
            vec![0u8; (self.draw_stride as usize).max(1) * snapshot.draws().len().max(1)];
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
                    resource: globals.as_entire_binding(),
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
        let geometry: Vec<(wgpu::Buffer, wgpu::Buffer, u32)> = snapshot
            .meshes()
            .iter()
            .map(|mesh| {
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
                (vertices, indices, mesh.indices().len() as u32)
            })
            .collect();

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
                let (vertices, indices, count) = &geometry[item.mesh];
                if *count == 0 {
                    continue;
                }
                pass.set_bind_group(0, &bindings, &[(index as u64 * self.draw_stride) as u32]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
        }

        let colour_read = self.readback(&mut encoder, &colour, extent, 4);
        let pick_read = self.readback(&mut encoder, &pick, extent, 4);
        self.queue.submit(Some(encoder.finish()));

        let colour = self.take(colour_read, width, height, 4)?;
        let picks = self.take(pick_read, width, height, 4)?;

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

    /// Copies a target into a buffer a host can map.
    ///
    /// Rows are padded to the alignment a copy demands, which is why the result
    /// is unpacked again rather than used as it lands.
    fn readback(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        extent: wgpu::Extent3d,
        bytes_per_pixel: u32,
    ) -> wgpu::Buffer {
        let padded = align_to(
            u64::from(extent.width * bytes_per_pixel),
            u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
        );
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ferritecad viewport readback"),
            size: padded * u64::from(extent.height),
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
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(extent.height),
                },
            },
            extent,
        );
        buffer
    }

    /// Waits for a readback and strips the row padding back out.
    fn take(
        &self,
        buffer: wgpu::Buffer,
        width: u32,
        height: u32,
        bytes_per_pixel: u32,
    ) -> Result<Vec<u8>> {
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| CadError::kernel(format!("waiting for the frame: {error}")))?;
        receiver
            .recv()
            .map_err(|error| CadError::kernel(format!("the frame was never read back: {error}")))?
            .map_err(|error| CadError::kernel(format!("reading the frame back: {error}")))?;

        let row = width * bytes_per_pixel;
        let padded = align_to(
            u64::from(row),
            u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
        );
        let mapped = slice
            .get_mapped_range()
            .map_err(|error| CadError::kernel(format!("mapping the frame: {error}")))?;
        let mut out = Vec::with_capacity((row * height) as usize);
        for line in 0..height as usize {
            let at = line * padded as usize;
            out.extend_from_slice(&mapped[at..at + row as usize]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(out)
    }
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
