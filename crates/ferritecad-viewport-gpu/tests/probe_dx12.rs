// SPDX-License-Identifier: MIT
//
// Temporary: isolates which part of the face target a backend refuses.

#![allow(clippy::panic)]

const HEAD: &str = r#"
struct Globals { view_projection: mat4x4<f32>, a: u32, b: u32, c: u32, d: u32 };
@group(0) @binding(0) var<uniform> globals: Globals;
"#;

const STORAGE: &str = "@group(0) @binding(1) var<storage, read> faces: array<u32>;\n";

fn source(primitive_index: bool, storage: bool, targets: usize, varyings: bool) -> String {
    let mut s = String::new();
    if primitive_index {
        s.push_str("enable primitive_index;\n");
    }
    s.push_str(HEAD);
    if storage {
        s.push_str(STORAGE);
    }
    if varyings {
        s.push_str(
            "struct VertexOut {\n@builtin(position) clip: vec4<f32>,\n\
             @location(0) normal: vec3<f32>,\n};\n\
             @vertex fn vertex_main(@builtin(vertex_index) i: u32) -> VertexOut {\n\
             var out: VertexOut;\n\
             out.clip = globals.view_projection * vec4<f32>(f32(i), 0.0, 0.0, 1.0);\n\
             out.normal = vec3<f32>(f32(i), 0.0, 1.0);\n\
             return out;\n}\n",
        );
    } else {
        s.push_str(
            "@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {\n\
             return globals.view_projection * vec4<f32>(f32(i), 0.0, 0.0, 1.0);\n}\n",
        );
    }
    s.push_str("struct Out {\n@location(0) colour: vec4<f32>,\n");
    if targets > 1 {
        s.push_str("@location(1) pick: u32,\n");
    }
    if targets > 2 {
        s.push_str("@location(2) face: u32,\n");
    }
    s.push_str("};\n@fragment fn fragment_main(");
    let mut arguments: Vec<&str> = Vec::new();
    if varyings {
        arguments.push("in: VertexOut");
    }
    if primitive_index {
        arguments.push("@builtin(primitive_index) triangle: u32");
    }
    s.push_str(&arguments.join(", "));
    s.push_str(") -> Out {\nvar out: Out;\n");
    if varyings {
        s.push_str("out.colour = vec4<f32>(in.normal, 1.0);\n");
    } else {
        s.push_str("out.colour = vec4<f32>(1.0);\n");
    }
    if targets > 1 {
        s.push_str("out.pick = 1u;\n");
    }
    if targets > 2 {
        let value = if storage && primitive_index {
            "faces[triangle]"
        } else if storage {
            "faces[0]"
        } else {
            "2u"
        };
        s.push_str(&format!("out.face = {value};\n"));
    }
    s.push_str("return out;\n}\n");
    s
}

#[test]
fn probe() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let mut report = String::new();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        panic!("PROBE: no adapter");
    };
    report.push_str(&format!(
        "\nadapter {:?} {} primitive_index={}\n",
        adapter.get_info().backend,
        adapter.get_info().name,
        adapter.features().contains(wgpu::Features::PRIMITIVE_INDEX)
    ));
    let Ok((device, _queue)) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("probe"),
        required_features: wgpu::Features::PRIMITIVE_INDEX,
        ..Default::default()
    })) else {
        panic!("PROBE:{report}no device with PRIMITIVE_INDEX");
    };
    device.on_uncaptured_error(std::sync::Arc::new(|error| {
        eprintln!("PROBE uncaptured: {error}")
    }));

    for primitive_index in [false, true] {
        for storage in [false, true] {
            for (targets, varyings) in
                [(1usize, false), (3, false), (1, true), (3, true)]
            {
                let label = format!(
                    "primitive_index={primitive_index} storage={storage} targets={targets} varyings={varyings}"
                );
                let validation_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
                let internal_scope = device.push_error_scope(wgpu::ErrorFilter::Internal);
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("probe module"),
                    source: wgpu::ShaderSource::Wgsl(
                        source(primitive_index, storage, targets, varyings).into(),
                    ),
                });
                let mut entries = vec![wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }];
                if storage {
                    entries.push(wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    });
                }
                let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &entries,
                });
                let pipeline_layout =
                    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: None,
                        bind_group_layouts: &[Some(&layout)],
                        immediate_size: 0,
                    });
                let mut colour_targets = vec![Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })];
                for _ in 1..targets {
                    colour_targets.push(Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R32Uint,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }));
                }
                let _pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("probe pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vertex_main"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some("fragment_main"),
                        compilation_options: Default::default(),
                        targets: &colour_targets,
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                });
                let internal = pollster::block_on(internal_scope.pop());
                let validation = pollster::block_on(validation_scope.pop());
                match (internal, validation) {
                    (None, None) => report.push_str(&format!("ok    {label}\n")),
                    (a, b) => report.push_str(&format!("FAIL  {label}: {a:?} / {b:?}\n")),
                }
            }
        }
    }

    // Fails on purpose: a passing test's output is captured, and the report
    // is the whole point of this branch.
    panic!("PROBE:{report}");
}

/// The real shader, built with the real layout, then reduced one element at a
/// time. The synthetic matrix above passes everywhere, so what fails is
/// something the real pipeline has and it does not.
#[test]
fn replica() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let mut report = String::new();
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        panic!("REPLICA: no adapter");
    };
    report.push_str(&format!(
        "\nadapter {:?} {}\n",
        adapter.get_info().backend,
        adapter.get_info().name
    ));
    let Ok((device, _queue)) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("replica"),
        required_features: wgpu::Features::PRIMITIVE_INDEX,
        ..Default::default()
    })) else {
        panic!("REPLICA:{report}no device with PRIMITIVE_INDEX");
    };

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("real shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../src/shader.wgsl").into()),
    });

    for identities in [true, false] {
        for depth in [true, false] {
            for vertices in [true, false] {
                for dynamic in [true, false] {
                    let label = format!(
                        "identities={identities} depth={depth} vertices={vertices} dynamic={dynamic}"
                    );
                    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
                    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);

                    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: None,
                        entries: &[
                            wgpu::BindGroupLayoutEntry {
                                binding: 0,
                                visibility: wgpu::ShaderStages::VERTEX
                                    | wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Uniform,
                                    has_dynamic_offset: false,
                                    min_binding_size: None,
                                },
                                count: None,
                            },
                            wgpu::BindGroupLayoutEntry {
                                binding: 1,
                                visibility: wgpu::ShaderStages::VERTEX
                                    | wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Uniform,
                                    has_dynamic_offset: dynamic,
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
                    let pipeline_layout =
                        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                            label: None,
                            bind_group_layouts: &[Some(&layout)],
                            immediate_size: 0,
                        });

                    let mut targets = vec![Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })];
                    if identities {
                        for _ in 0..2 {
                            targets.push(Some(wgpu::ColorTargetState {
                                format: wgpu::TextureFormat::R32Uint,
                                blend: None,
                                write_mask: wgpu::ColorWrites::ALL,
                            }));
                        }
                    }
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
                        label: Some("replica pipeline"),
                        layout: Some(&pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: &module,
                            entry_point: Some("vertex_main"),
                            compilation_options: Default::default(),
                            buffers: if vertices { &buffers } else { &[] },
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &module,
                            entry_point: Some(if identities {
                                "fragment_main"
                            } else {
                                "fragment_colour"
                            }),
                            compilation_options: Default::default(),
                            targets: &targets,
                        }),
                        primitive: wgpu::PrimitiveState {
                            cull_mode: None,
                            ..Default::default()
                        },
                        depth_stencil: depth.then(|| wgpu::DepthStencilState {
                            format: wgpu::TextureFormat::Depth32Float,
                            depth_write_enabled: Some(true),
                            depth_compare: Some(wgpu::CompareFunction::Less),
                            stencil: Default::default(),
                            bias: Default::default(),
                        }),
                        multisample: Default::default(),
                        multiview_mask: None,
                        cache: None,
                    });

                    let internal = pollster::block_on(internal.pop());
                    let validation = pollster::block_on(validation.pop());
                    match (internal, validation) {
                        (None, None) => report.push_str(&format!("ok    {label}\n")),
                        (a, b) => report.push_str(&format!("FAIL  {label}: {a:?} / {b:?}\n")),
                    }
                }
            }
        }
    }

    panic!("REPLICA:{report}");
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
