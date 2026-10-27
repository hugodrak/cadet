mod camera;
mod mesh;

pub use camera::Camera;
pub use mesh::{MeshData, Vertex};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;

#[derive(Clone, Copy)]
pub struct Style {
    pub bg_top: [f32; 4],
    pub bg_bottom: [f32; 4],
    pub body: [f32; 4],
    pub edge: [f32; 4],
    pub grid_minor: [f32; 4],
    pub grid_major: [f32; 4],
    pub axis_x: [f32; 4],
    pub axis_y: [f32; 4],
    pub show_grid: bool,
    pub show_edges: bool,
    pub show_body: bool,
    /// Grid plane as origin, u, v.
    pub grid_plane: [[f32; 3]; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    light: [f32; 4],
    grid: [f32; 4],
    plane: [[f32; 4]; 3],
    colors: [[f32; 4]; 8],
}

struct Targets {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolve: wgpu::TextureView,
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    edges: wgpu::Buffer,
    edge_count: u32,
}

pub struct Viewport {
    globals: wgpu::Buffer,
    bind: wgpu::BindGroup,
    bg: wgpu::RenderPipeline,
    body: wgpu::RenderPipeline,
    edge: wgpu::RenderPipeline,
    grid: wgpu::RenderPipeline,
    targets: Option<Targets>,
    mesh: Option<GpuMesh>,
}

impl Viewport {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let pipeline = |vs: &str, fs: &str, buffers: &[Option<wgpu::VertexBufferLayout>], topology, depth_write, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(vs),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        use wgpu::PrimitiveTopology::*;
        let alpha = Some(wgpu::BlendState::ALPHA_BLENDING);
        let mesh_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
        };
        let edge_layout = wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3],
        };

        Self {
            bg: pipeline("vs_bg", "fs_bg", &[], TriangleList, false, None),
            body: pipeline("vs_mesh", "fs_mesh", &[Some(mesh_layout)], TriangleList, true, None),
            edge: pipeline("vs_edge", "fs_edge", &[Some(edge_layout)], LineList, false, alpha),
            grid: pipeline("vs_grid", "fs_grid", &[], TriangleList, false, alpha),
            globals,
            bind,
            targets: None,
            mesh: None,
        }
    }

    pub fn set_mesh(&mut self, device: &wgpu::Device, mesh: &MeshData) {
        if mesh.indices.is_empty() {
            self.mesh = None;
            return;
        }
        let buf = |contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents, usage })
        };
        self.mesh = Some(GpuMesh {
            vertices: buf(bytemuck::cast_slice(&mesh.vertices), wgpu::BufferUsages::VERTEX),
            indices: buf(bytemuck::cast_slice(&mesh.indices), wgpu::BufferUsages::INDEX),
            index_count: mesh.indices.len() as u32,
            edges: buf(bytemuck::cast_slice(&mesh.edges), wgpu::BufferUsages::VERTEX),
            edge_count: mesh.edges.len() as u32,
        });
    }

    /// Renders into an offscreen texture. Returns the view and whether it was recreated.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: [u32; 2],
        camera: &Camera,
        style: &Style,
    ) -> (&wgpu::TextureView, bool) {
        let size = [size[0].max(1), size[1].max(1)];
        let recreated = self.targets.as_ref().is_none_or(|t| t.size != size);
        if recreated {
            self.targets = Some(create_targets(device, size));
        }

        let step = 10f32.powf((camera.dist * 0.02).max(0.1).log10().floor());
        let s = style;
        let globals = Globals {
            view_proj: camera.view_proj(size[0] as f32 / size[1] as f32).to_cols_array_2d(),
            eye: camera.eye().extend(1.0).into(),
            light: camera.key_light().extend(0.0).into(),
            grid: [step, camera.dist * 6.0, 0.0, 0.0],
            plane: s.grid_plane.map(|[x, y, z]| [x, y, z, 0.0]),
            colors: [s.bg_top, s.bg_bottom, s.body, s.edge, s.grid_minor, s.grid_major, s.axis_x, s.axis_y],
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        let t = self.targets.as_ref().unwrap();
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.msaa,
                    depth_slice: None,
                    resolve_target: Some(&t.resolve),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
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
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_pipeline(&self.bg);
            pass.draw(0..3, 0..1);
            if let Some(m) = self.mesh.as_ref().filter(|_| style.show_body) {
                pass.set_pipeline(&self.body);
                pass.set_vertex_buffer(0, m.vertices.slice(..));
                pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..m.index_count, 0, 0..1);
                if style.show_edges && m.edge_count > 0 {
                    pass.set_pipeline(&self.edge);
                    pass.set_vertex_buffer(0, m.edges.slice(..));
                    pass.draw(0..m.edge_count, 0..1);
                }
            }
            if style.show_grid {
                pass.set_pipeline(&self.grid);
                pass.draw(0..6, 0..1);
            }
        }
        queue.submit([enc.finish()]);
        (&self.targets.as_ref().unwrap().resolve, recreated)
    }
}

fn create_targets(device: &wgpu::Device, size: [u32; 2]) -> Targets {
    let tex = |format, samples, usage| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };
    use wgpu::TextureUsages as U;
    Targets {
        size,
        msaa: tex(COLOR_FORMAT, SAMPLES, U::RENDER_ATTACHMENT),
        depth: tex(DEPTH_FORMAT, SAMPLES, U::RENDER_ATTACHMENT),
        resolve: tex(COLOR_FORMAT, 1, U::RENDER_ATTACHMENT | U::TEXTURE_BINDING),
    }
}
