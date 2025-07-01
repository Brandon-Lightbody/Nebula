use wgpu::{
    Device, RenderPipeline, SurfaceConfiguration, VertexBufferLayout, VertexAttribute,
    VertexStepMode, VertexFormat, BindGroupLayout, PipelineLayout, ShaderModule,
};

pub struct GpuResources {
    pub pipeline: RenderPipeline,
}

impl GpuResources {
    pub fn new(
        device: &Device,
        config: &SurfaceConfiguration,
        bind_group_layout: &BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/shader.wgsl"));

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = create_pipeline(device, config, &shader, &pipeline_layout);
        Self { pipeline }
    }
}

fn create_pipeline(
    device: &Device,
    config: &SurfaceConfiguration,
    shader: &ShaderModule,
    pipeline_layout: &PipelineLayout,
) -> RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Text Render Pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[VertexBufferLayout {
    array_stride: std::mem::size_of::<[f32; 4]>() as u64,
    step_mode: VertexStepMode::Vertex,
    attributes: &[
        VertexAttribute { // position
            format: VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        },
        VertexAttribute { // tex_coord
            format: VertexFormat::Float32x2,
            offset: std::mem::size_of::<[f32; 2]>() as u64,
            shader_location: 1,
        },
    ],
}],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: config.format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
            strip_index_format: None,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
        cache: None,
    })
}