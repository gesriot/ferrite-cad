// SPDX-License-Identifier: MIT
//
// Temporary: isolates which part of the face target a backend refuses.

#![allow(clippy::panic)]

/// The window pipeline, whose colour format no other probe has used.
///
/// The first failing run was the lib gate, where one renderer exists and
/// nothing runs beside it. What that gate does and no probe has done is build
/// a pipeline for a surface format: `Bgra8UnormSrgb`.
#[test]
fn the_window_pipeline_three_times() {
    let mut report = format!(
        "shader carries the face target: {}\n",
        include_str!("../src/shader.wgsl").contains("enable primitive_index")
    );
    for attempt in 1..=3 {
        let mut renderer = match ferritecad_viewport_gpu::Renderer::new() {
            Ok(renderer) => renderer,
            Err(error) => {
                report.push_str(&format!("{attempt}: new refused: {error}\n"));
                continue;
            }
        };
        let mut builder = ferritecad_viewport::SnapshotBuilder::new();
        let mesh = ferritecad_kernel::Mesh {
            positions: vec![-5.0, 0.0, -5.0, 5.0, 0.0, -5.0, 5.0, 0.0, 5.0],
            normals: vec![0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0],
            indices: vec![0, 1, 2],
            faces: vec![ferritecad_kernel::MeshFaceRange {
                face: ferritecad_kernel::SubShapeHandle::new(
                    ferritecad_kernel::ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1),
                    ferritecad_kernel::SubShapeKind::Face,
                    0,
                ),
                first_index: 0,
                index_count: 3,
            }],
        };
        let definition = builder.add_mesh(&mesh).expect("packs");
        builder
            .place(
                definition,
                None,
                &ferritecad_types::Transform::IDENTITY,
                [0.0, 1.0, 0.0],
            )
            .expect("places");
        let snapshot = std::sync::Arc::new(builder.build());
        let mut camera = ferritecad_viewport::Camera::new();
        camera.resize(64, 64);
        camera
            .frame(snapshot.bounds().expect("geometry"))
            .expect("frames");
        let prepared = renderer.prepare(snapshot).expect("uploads");

        // Exactly what the failing gate does, including its colour format.
        match renderer.draw_into_for_probe(&prepared, &camera, wgpu::TextureFormat::Bgra8UnormSrgb)
        {
            Ok(()) => report.push_str(&format!("{attempt}: ok\n")),
            Err(error) => report.push_str(&format!("{attempt}: {error}\n")),
        }
    }
    panic!("WINDOW:\n{report}");
}

/// The real shader, reduced one construct at a time.
///
/// The synthetic matrix passes everywhere and every pipeline built from the
/// real module fails on one backend, so the difference is inside the module.
#[test]
fn reduce_the_real_shader() {
    let real = include_str!("../src/shader.wgsl");
    let without_enable = real.replace("enable primitive_index;", "");
    let no_lookup = real.replace(
        "return faces[draw.first_triangle + triangle];",
        "return draw.first_triangle + triangle;",
    );
    let no_primitive = without_enable
        .replace(
            "fn fragment_main(in: VertexOut, @builtin(primitive_index) triangle: u32)",
            "fn fragment_main(in: VertexOut)",
        )
        .replace(
            "fn fragment_colour(in: VertexOut, @builtin(primitive_index) triangle: u32)",
            "fn fragment_colour(in: VertexOut)",
        )
        .replace("let face = face_of(triangle);", "let face = face_of(0u);")
        .replace("return shade(in, face_of(triangle));", "return shade(in, face_of(0u));");
    let neither = no_primitive.replace(
        "return faces[draw.first_triangle + triangle];",
        "return draw.first_triangle + triangle;",
    );

    let variants = [
        ("the real shader", real.to_string()),
        ("without the storage lookup", no_lookup),
        ("without primitive_index", no_primitive),
        ("without either", neither),
    ];

    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let mut report = String::new();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        panic!("REDUCE: no adapter");
    };
    report.push_str(&format!(
        "\nadapter {:?} {}\n",
        adapter.get_info().backend,
        adapter.get_info().name
    ));
    let Ok((device, _queue)) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("reduce"),
        required_features: wgpu::Features::PRIMITIVE_INDEX,
        ..Default::default()
    })) else {
        panic!("REDUCE:{report}no device with PRIMITIVE_INDEX");
    };

    for (name, source) in variants {
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
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
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(96),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let targets = [
            Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
            Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::R32Uint,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
            Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::R32Uint,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
        ];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: 24,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 12,
                    shader_location: 1,
                },
            ],
        })];
        let _pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("reduce pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let internal = pollster::block_on(internal.pop());
        let validation = pollster::block_on(validation.pop());
        match (internal, validation) {
            (None, None) => report.push_str(&format!("ok    {name}\n")),
            (a, b) => report.push_str(&format!("FAIL  {name}: {a:?} / {b:?}\n")),
        }
    }

    panic!("REDUCE:{report}");
}
