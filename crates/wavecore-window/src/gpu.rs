use bytemuck::{Pod, Zeroable};
use wavecore_layout::Rect;
use wavecore_pixels::{Rgba, Surface};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use wavecore_render::{CompositorFrame, WebGlCommand};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    alpha: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct WebGlVertex {
    position: [f32; 2],
    color: [f32; 4],
}

struct GpuWebGlDraw {
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
    scissor: (u32, u32, u32, u32),
}

const COMPOSITOR_TILE_SIZE: f32 = 512.0;
const TILE_CACHE_TTL_FRAMES: u64 = 180;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TileKey {
    layer_id: u64,
    x: i32,
    y: i32,
}

struct CachedTile {
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    fingerprint: u64,
    width: u32,
    height: u32,
    last_used_frame: u64,
}

struct GpuTileDraw {
    key: TileKey,
    vertex_buffer: wgpu::Buffer,
}

struct GpuLayer {
    tiles: Vec<GpuTileDraw>,
    webgl_draws: Vec<GpuWebGlDraw>,
}

pub struct GpuCompositor {
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    _adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    webgl_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    raster_template: Surface,
    adapter_name: String,
    tile_cache: HashMap<TileKey, CachedTile>,
    frame_counter: u64,
}

impl GpuCompositor {
    pub fn new(
        window: &minifb::Window,
        width: usize,
        height: usize,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::default();

        // SAFETY: BrowserWindow stores the GPU compositor before the minifb window,
        // so the surface is dropped first. minifb 0.28 exposes raw-window-handle 0.6.
        let target = unsafe { wgpu::SurfaceTargetUnsafe::from_window(window) }
            .map_err(|e| format!("raw window handle: {e}"))?;
        let surface: wgpu::Surface<'static> =
            unsafe { instance.create_surface_unsafe(target) }
                .map_err(|e| format!("create wgpu surface: {e}"))?;

        let adapter = pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            },
        ))
        .ok_or_else(|| "no compatible GPU adapter found".to_string())?;

        let adapter_info = adapter.get_info();
        let adapter_name = format!(
            "{} ({:?}/{:?})",
            adapter_info.name, adapter_info.backend, adapter_info.device_type
        );

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("WaveCore GPU Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
            },
            None,
        ))
        .map_err(|e| format!("request GPU device: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| "surface has no supported texture formats".to_string())?;
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
            wgpu::PresentMode::Fifo
        } else {
            *caps
                .present_modes
                .first()
                .ok_or_else(|| "surface has no present mode".to_string())?
        };
        let alpha_mode = *caps
            .alpha_modes
            .first()
            .ok_or_else(|| "surface has no alpha mode".to_string())?;

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1) as u32,
            height: height.max(1) as u32,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("WaveCore Layer BGL"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(
                            wgpu::SamplerBindingType::Filtering,
                        ),
                        count: None,
                    },
                ],
            });

        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("WaveCore Compositor Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("WaveCore Compositor Shader"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
) -> VertexOut {
    var out: VertexOut;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    out.alpha = alpha;
    return out;
}

@group(0) @binding(0)
var layer_texture: texture_2d<f32>;
@group(0) @binding(1)
var layer_sampler: sampler;

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(layer_texture, layer_sampler, in.uv);
    return vec4<f32>(texel.rgb, texel.a * in.alpha);
}
"#
                .into(),
            ),
        });

        const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32];

        let pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("WaveCore GPU Compositor"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &ATTRIBUTES,
                    }],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let webgl_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("WaveCore WebGL Fixed Pipeline Shader"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
) -> VertexOut {
    var out: VertexOut;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#
                .into(),
            ),
        });

        const WEBGL_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

        let webgl_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("WaveCore WebGL Draw Pipeline"),
                layout: Some(&device.create_pipeline_layout(
                    &wgpu::PipelineLayoutDescriptor {
                        label: Some("WaveCore WebGL Pipeline Layout"),
                        bind_group_layouts: &[],
                        push_constant_ranges: &[],
                    },
                )),
                vertex: wgpu::VertexState {
                    module: &webgl_shader,
                    entry_point: "vs_main",
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<WebGlVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &WEBGL_ATTRIBUTES,
                    }],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &webgl_shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("WaveCore Layer Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Ok(Self {
            _instance: instance,
            surface,
            _adapter: adapter,
            device,
            queue,
            config,
            pipeline,
            webgl_pipeline,
            bind_group_layout,
            sampler,
            raster_template: Surface::new(1, 1),
            adapter_name,
            tile_cache: HashMap::new(),
            frame_counter: 0,
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width as u32;
        self.config.height = height as u32;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn present(
        &mut self,
        frame: &CompositorFrame,
        scroll_y: f32,
    ) -> Result<(), String> {
        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                self.surface
                    .get_current_texture()
                    .map_err(|e| format!("acquire GPU surface after reconfigure: {e}"))?
            }
            Err(wgpu::SurfaceError::Timeout) => {
                return Ok(());
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                return Err("GPU surface out of memory".to_string());
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut sorted_layers = frame.layers.clone();
        sorted_layers.sort_by_key(|layer| layer.z_index);

        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            width: self.config.width as f32,
            height: self.config.height as f32,
        };

        self.frame_counter = self.frame_counter.wrapping_add(1);
        let current_frame = self.frame_counter;

        let mut gpu_layers = Vec::new();
        for (layer_index, layer) in sorted_layers.iter().enumerate() {
            let screen_bounds = Rect {
                x: layer.bounds.x,
                y: layer.bounds.y - scroll_y,
                width: layer.bounds.width,
                height: layer.bounds.height,
            };
            let Some(visible_screen) = screen_bounds.intersection(&viewport) else {
                continue;
            };
            if visible_screen.width <= 0.0 || visible_screen.height <= 0.0 {
                continue;
            }

            let visible_doc = Rect {
                x: visible_screen.x,
                y: visible_screen.y + scroll_y,
                width: visible_screen.width,
                height: visible_screen.height,
            };
            let layer_id = layer
                .node_id
                .unwrap_or(0x8000_0000_0000_0000u64 | layer_index as u64);
            let fingerprint = layer_fingerprint(layer);

            let min_tile_x =
                ((visible_doc.x - layer.bounds.x) / COMPOSITOR_TILE_SIZE).floor() as i32;
            let max_tile_x = ((visible_doc.x + visible_doc.width - layer.bounds.x)
                / COMPOSITOR_TILE_SIZE)
                .floor() as i32;
            let min_tile_y =
                ((visible_doc.y - layer.bounds.y) / COMPOSITOR_TILE_SIZE).floor() as i32;
            let max_tile_y = ((visible_doc.y + visible_doc.height - layer.bounds.y)
                / COMPOSITOR_TILE_SIZE)
                .floor() as i32;

            let mut tile_draws = Vec::new();
            for tile_y in min_tile_y..=max_tile_y {
                for tile_x in min_tile_x..=max_tile_x {
                    let tile_doc = compositor_tile_rect(layer.bounds, tile_x, tile_y);
                    if tile_doc.width <= 0.0 || tile_doc.height <= 0.0 {
                        continue;
                    }

                    let key = TileKey {
                        layer_id,
                        x: tile_x,
                        y: tile_y,
                    };
                    let tex_w = tile_doc.width.ceil().max(1.0) as u32;
                    let tex_h = tile_doc.height.ceil().max(1.0) as u32;
                    let reusable = self
                        .tile_cache
                        .get(&key)
                        .map(|tile| {
                            tile.fingerprint == fingerprint
                                && tile.width == tex_w
                                && tile.height == tex_h
                        })
                        .unwrap_or(false);

                    if !reusable {
                        let mut raster = self
                            .raster_template
                            .blank_like(tex_w, tex_h, Rgba(0, 0, 0, 0));
                        raster.paint_offset(
                            &layer.commands,
                            -tile_doc.x,
                            -tile_doc.y,
                        );
                        let rgba = surface_rgba_bytes(&raster);
                        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                            label: Some("WaveCore Cached Compositor Tile"),
                            size: wgpu::Extent3d {
                                width: tex_w,
                                height: tex_h,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba8UnormSrgb,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::COPY_DST,
                            view_formats: &[],
                        });
                        write_texture_rgba(
                            &self.queue,
                            &texture,
                            tex_w,
                            tex_h,
                            &rgba,
                        );
                        let texture_view =
                            texture.create_view(&wgpu::TextureViewDescriptor::default());
                        let bind_group =
                            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("WaveCore Cached Tile Bind Group"),
                                layout: &self.bind_group_layout,
                                entries: &[
                                    wgpu::BindGroupEntry {
                                        binding: 0,
                                        resource: wgpu::BindingResource::TextureView(&texture_view),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 1,
                                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                                    },
                                ],
                            });
                        self.tile_cache.insert(
                            key,
                            CachedTile {
                                _texture: texture,
                                _view: texture_view,
                                bind_group,
                                fingerprint,
                                width: tex_w,
                                height: tex_h,
                                last_used_frame: current_frame,
                            },
                        );
                    } else if let Some(tile) = self.tile_cache.get_mut(&key) {
                        tile.last_used_frame = current_frame;
                    }

                    let tile_screen = Rect {
                        x: tile_doc.x,
                        y: tile_doc.y - scroll_y,
                        width: tile_doc.width,
                        height: tile_doc.height,
                    };
                    let Some(draw_rect) = tile_screen.intersection(&viewport) else {
                        continue;
                    };

                    let u0 = ((draw_rect.x - tile_screen.x) / tile_screen.width)
                        .clamp(0.0, 1.0);
                    let v0 = ((draw_rect.y - tile_screen.y) / tile_screen.height)
                        .clamp(0.0, 1.0);
                    let u1 = ((draw_rect.x + draw_rect.width - tile_screen.x)
                        / tile_screen.width)
                        .clamp(0.0, 1.0);
                    let v1 = ((draw_rect.y + draw_rect.height - tile_screen.y)
                        / tile_screen.height)
                        .clamp(0.0, 1.0);

                    let vertices = quad_vertices_uv(
                        draw_rect,
                        self.config.width as f32,
                        self.config.height as f32,
                        [u0, v0, u1, v1],
                        layer.opacity,
                    );
                    let vertex_buffer = {
                        use wgpu::util::DeviceExt;
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("WaveCore Cached Tile Vertices"),
                                contents: bytemuck::cast_slice(&vertices),
                                usage: wgpu::BufferUsages::VERTEX,
                            })
                    };
                    tile_draws.push(GpuTileDraw {
                        key,
                        vertex_buffer,
                    });
                }
            }

            let webgl_draws = build_webgl_draws(
                &self.device,
                &layer.webgl_commands,
                layer.bounds,
                scroll_y,
                self.config.width as f32,
                self.config.height as f32,
                layer.opacity,
            );

            gpu_layers.push(GpuLayer {
                tiles: tile_draws,
                webgl_draws,
            });
        }

        self.tile_cache.retain(|_, tile| {
            current_frame.saturating_sub(tile.last_used_frame) <= TILE_CACHE_TTL_FRAMES
        });

        let mut encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("WaveCore Compositor Encoder"),
                });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("WaveCore Compositor Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 13.0 / 255.0,
                            g: 17.0 / 255.0,
                            b: 23.0 / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);

            for layer in &gpu_layers {
                pass.set_pipeline(&self.pipeline);
                for tile_draw in &layer.tiles {
                    let Some(tile) = self.tile_cache.get(&tile_draw.key) else {
                        continue;
                    };
                    pass.set_bind_group(0, &tile.bind_group, &[]);
                    pass.set_vertex_buffer(0, tile_draw.vertex_buffer.slice(..));
                    pass.draw(0..6, 0..1);
                }

                if !layer.webgl_draws.is_empty() {
                    pass.set_pipeline(&self.webgl_pipeline);
                    for draw in &layer.webgl_draws {
                        let (x, y, width, height) = draw.scissor;
                        if width == 0 || height == 0 {
                            continue;
                        }
                        pass.set_scissor_rect(x, y, width, height);
                        pass.set_vertex_buffer(0, draw.vertex_buffer.slice(..));
                        pass.draw(0..draw.vertex_count, 0..1);
                    }
                    pass.set_scissor_rect(0, 0, self.config.width, self.config.height);
                }
            }
        }

        self.queue.submit(Some(encoder.finish()));
        output.present();
        Ok(())
    }
}



fn layer_fingerprint(layer: &wavecore_render::CompositorLayer) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", layer.commands).hash(&mut hasher);
    layer.bounds.x.to_bits().hash(&mut hasher);
    layer.bounds.y.to_bits().hash(&mut hasher);
    layer.bounds.width.to_bits().hash(&mut hasher);
    layer.bounds.height.to_bits().hash(&mut hasher);
    hasher.finish()
}

fn compositor_tile_rect(bounds: Rect, tile_x: i32, tile_y: i32) -> Rect {
    let x = bounds.x + tile_x as f32 * COMPOSITOR_TILE_SIZE;
    let y = bounds.y + tile_y as f32 * COMPOSITOR_TILE_SIZE;
    let right = (x + COMPOSITOR_TILE_SIZE).min(bounds.x + bounds.width);
    let bottom = (y + COMPOSITOR_TILE_SIZE).min(bounds.y + bounds.height);
    Rect {
        x,
        y,
        width: (right - x).max(0.0),
        height: (bottom - y).max(0.0),
    }
}

fn build_webgl_draws(
    device: &wgpu::Device,
    commands: &[WebGlCommand],
    bounds: Rect,
    scroll_y: f32,
    viewport_width: f32,
    viewport_height: f32,
    layer_opacity: f32,
) -> Vec<GpuWebGlDraw> {
    use wgpu::util::DeviceExt;

    #[derive(Clone, Copy)]
    struct AttribState {
        buffer_id: Option<u32>,
        size: usize,
        stride: usize,
        offset: usize,
        enabled: bool,
        constant: [f32; 4],
    }

    if commands.is_empty() || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return Vec::new();
    }

    let mut draws = Vec::new();
    let mut clear_color = [0.0, 0.0, 0.0, 0.0];
    let mut buffers: HashMap<u32, Vec<f32>> = HashMap::new();
    let mut element_buffers: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut bound_element_buffer: Option<u32> = None;
    let mut attributes: HashMap<u32, AttribState> = HashMap::new();
    let mut viewport = [0.0f32, 0.0, bounds.width, bounds.height];

    attributes.insert(
        0,
        AttribState {
            buffer_id: None,
            size: 2,
            stride: 0,
            offset: 0,
            enabled: false,
            constant: [0.0, 0.0, 0.0, 1.0],
        },
    );
    attributes.insert(
        1,
        AttribState {
            buffer_id: None,
            size: 4,
            stride: 0,
            offset: 0,
            enabled: false,
            constant: [1.0, 1.0, 1.0, 1.0],
        },
    );

    for command in commands {
        match command {
            WebGlCommand::Viewport { x, y, width, height } => {
                viewport = [
                    *x as f32,
                    *y as f32,
                    (*width).max(0) as f32,
                    (*height).max(0) as f32,
                ];
            }
            WebGlCommand::ClearColor(color) => clear_color = *color,
            WebGlCommand::Clear { mask } if mask & 0x4000 != 0 => {
                let screen = Rect {
                    x: bounds.x,
                    y: bounds.y - scroll_y,
                    width: bounds.width,
                    height: bounds.height,
                };
                let vertices = solid_quad_vertices(
                    screen,
                    viewport_width,
                    viewport_height,
                    [
                        clear_color[0],
                        clear_color[1],
                        clear_color[2],
                        clear_color[3] * layer_opacity,
                    ],
                );
                let vertex_buffer = device.create_buffer_init(
                    &wgpu::util::BufferInitDescriptor {
                        label: Some("WaveCore WebGL Clear Vertices"),
                        contents: bytemuck::cast_slice(&vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    },
                );
                draws.push(GpuWebGlDraw {
                    vertex_buffer,
                    vertex_count: vertices.len() as u32,
                    scissor: layer_scissor(bounds, scroll_y, viewport_width, viewport_height),
                });
            }
            WebGlCommand::UploadArrayBuffer { id, data } => {
                buffers.insert(*id, data.clone());
            }
            WebGlCommand::UploadElementArrayBuffer { id, data } => {
                element_buffers.insert(*id, data.clone());
            }
            WebGlCommand::BindElementArrayBuffer(id) => {
                bound_element_buffer = *id;
            }
            WebGlCommand::VertexAttribPointer {
                index,
                size,
                stride_floats,
                offset_floats,
                buffer_id,
            } => {
                let entry = attributes.entry(*index).or_insert(AttribState {
                    buffer_id: None,
                    size: 4,
                    stride: 0,
                    offset: 0,
                    enabled: false,
                    constant: [0.0, 0.0, 0.0, 1.0],
                });
                entry.buffer_id = *buffer_id;
                entry.size = (*size as usize).clamp(1, 4);
                entry.stride = *stride_floats as usize;
                entry.offset = *offset_floats as usize;
            }
            WebGlCommand::EnableVertexAttribArray(index) => {
                attributes
                    .entry(*index)
                    .or_insert(AttribState {
                        buffer_id: None,
                        size: 4,
                        stride: 0,
                        offset: 0,
                        enabled: false,
                        constant: [0.0, 0.0, 0.0, 1.0],
                    })
                    .enabled = true;
            }
            WebGlCommand::VertexAttrib4f { index, value } => {
                attributes
                    .entry(*index)
                    .or_insert(AttribState {
                        buffer_id: None,
                        size: 4,
                        stride: 0,
                        offset: 0,
                        enabled: false,
                        constant: [0.0, 0.0, 0.0, 1.0],
                    })
                    .constant = *value;
            }
            WebGlCommand::DrawArrays { mode, first, count } if *mode == 0x0004 => {
                let Some(position_state) = attributes.get(&0).copied() else {
                    continue;
                };
                if !position_state.enabled {
                    continue;
                }
                let Some(position_buffer_id) = position_state.buffer_id else {
                    continue;
                };
                let Some(position_data) = buffers.get(&position_buffer_id) else {
                    continue;
                };

                let color_state = attributes.get(&1).copied().unwrap_or(AttribState {
                    buffer_id: None,
                    size: 4,
                    stride: 0,
                    offset: 0,
                    enabled: false,
                    constant: [1.0, 1.0, 1.0, 1.0],
                });

                let position_stride = if position_state.stride == 0 {
                    position_state.size
                } else {
                    position_state.stride
                };
                let color_stride = if color_state.stride == 0 {
                    color_state.size
                } else {
                    color_state.stride
                };

                let mut vertices = Vec::new();
                for vertex_index in *first as usize..(*first + *count) as usize {
                    let pos_base =
                        position_state.offset + vertex_index.saturating_mul(position_stride);
                    if pos_base >= position_data.len() {
                        break;
                    }
                    let x = position_data.get(pos_base).copied().unwrap_or(0.0);
                    let y = position_data.get(pos_base + 1).copied().unwrap_or(0.0);

                    let color = if color_state.enabled {
                        if let Some(color_buffer_id) = color_state.buffer_id {
                            if let Some(color_data) = buffers.get(&color_buffer_id) {
                                let base =
                                    color_state.offset + vertex_index.saturating_mul(color_stride);
                                [
                                    color_data.get(base).copied().unwrap_or(0.0),
                                    color_data.get(base + 1).copied().unwrap_or(0.0),
                                    color_data.get(base + 2).copied().unwrap_or(0.0),
                                    color_data.get(base + 3).copied().unwrap_or(1.0)
                                        * layer_opacity,
                                ]
                            } else {
                                color_state.constant
                            }
                        } else {
                            color_state.constant
                        }
                    } else {
                        [
                            color_state.constant[0],
                            color_state.constant[1],
                            color_state.constant[2],
                            color_state.constant[3] * layer_opacity,
                        ]
                    };

                    let vx = viewport[0] + ((x + 1.0) * 0.5) * viewport[2];
                    let vy = viewport[1] + (1.0 - (y + 1.0) * 0.5) * viewport[3];
                    let sx = bounds.x + vx;
                    let sy = bounds.y - scroll_y + vy;
                    let ndc_x = sx / viewport_width * 2.0 - 1.0;
                    let ndc_y = 1.0 - sy / viewport_height * 2.0;

                    vertices.push(WebGlVertex {
                        position: [ndc_x, ndc_y],
                        color,
                    });
                }

                if vertices.len() >= 3 {
                    let vertex_buffer = device.create_buffer_init(
                        &wgpu::util::BufferInitDescriptor {
                            label: Some("WaveCore WebGL Draw Vertices"),
                            contents: bytemuck::cast_slice(&vertices),
                            usage: wgpu::BufferUsages::VERTEX,
                        },
                    );
                    draws.push(GpuWebGlDraw {
                        vertex_buffer,
                        vertex_count: vertices.len() as u32,
                        scissor: layer_scissor(bounds, scroll_y, viewport_width, viewport_height),
                    });
                }
            }
            WebGlCommand::DrawElements {
                mode,
                count,
                element_type,
                offset_bytes,
            } if *mode == 0x0004 => {
                let Some(index_buffer_id) = bound_element_buffer else {
                    continue;
                };
                let Some(index_data) = element_buffers.get(&index_buffer_id) else {
                    continue;
                };
                let index_size = match *element_type {
                    0x1403 => 2usize,
                    0x1405 => 4usize,
                    _ => continue,
                };
                let start_index = (*offset_bytes as usize) / index_size;
                let end_index = start_index.saturating_add(*count as usize).min(index_data.len());

                let Some(position_state) = attributes.get(&0).copied() else {
                    continue;
                };
                if !position_state.enabled {
                    continue;
                }
                let Some(position_buffer_id) = position_state.buffer_id else {
                    continue;
                };
                let Some(position_data) = buffers.get(&position_buffer_id) else {
                    continue;
                };

                let color_state = attributes.get(&1).copied().unwrap_or(AttribState {
                    buffer_id: None,
                    size: 4,
                    stride: 0,
                    offset: 0,
                    enabled: false,
                    constant: [1.0, 1.0, 1.0, 1.0],
                });
                let position_stride = if position_state.stride == 0 {
                    position_state.size
                } else {
                    position_state.stride
                };
                let color_stride = if color_state.stride == 0 {
                    color_state.size
                } else {
                    color_state.stride
                };

                let mut vertices = Vec::new();
                for vertex_index in index_data[start_index..end_index].iter().copied() {
                    let vertex_index = vertex_index as usize;
                    let pos_base =
                        position_state.offset + vertex_index.saturating_mul(position_stride);
                    if pos_base >= position_data.len() {
                        continue;
                    }
                    let x = position_data.get(pos_base).copied().unwrap_or(0.0);
                    let y = position_data.get(pos_base + 1).copied().unwrap_or(0.0);

                    let color = if color_state.enabled {
                        if let Some(color_buffer_id) = color_state.buffer_id {
                            if let Some(color_data) = buffers.get(&color_buffer_id) {
                                let base =
                                    color_state.offset + vertex_index.saturating_mul(color_stride);
                                [
                                    color_data.get(base).copied().unwrap_or(0.0),
                                    color_data.get(base + 1).copied().unwrap_or(0.0),
                                    color_data.get(base + 2).copied().unwrap_or(0.0),
                                    color_data.get(base + 3).copied().unwrap_or(1.0)
                                        * layer_opacity,
                                ]
                            } else {
                                color_state.constant
                            }
                        } else {
                            color_state.constant
                        }
                    } else {
                        [
                            color_state.constant[0],
                            color_state.constant[1],
                            color_state.constant[2],
                            color_state.constant[3] * layer_opacity,
                        ]
                    };

                    let vx = viewport[0] + ((x + 1.0) * 0.5) * viewport[2];
                    let vy = viewport[1] + (1.0 - (y + 1.0) * 0.5) * viewport[3];
                    let sx = bounds.x + vx;
                    let sy = bounds.y - scroll_y + vy;
                    let ndc_x = sx / viewport_width * 2.0 - 1.0;
                    let ndc_y = 1.0 - sy / viewport_height * 2.0;

                    vertices.push(WebGlVertex {
                        position: [ndc_x, ndc_y],
                        color,
                    });
                }

                if vertices.len() >= 3 {
                    let vertex_buffer = device.create_buffer_init(
                        &wgpu::util::BufferInitDescriptor {
                            label: Some("WaveCore WebGL Indexed Vertices"),
                            contents: bytemuck::cast_slice(&vertices),
                            usage: wgpu::BufferUsages::VERTEX,
                        },
                    );
                    draws.push(GpuWebGlDraw {
                        vertex_buffer,
                        vertex_count: vertices.len() as u32,
                        scissor: layer_scissor(bounds, scroll_y, viewport_width, viewport_height),
                    });
                }
            }
            _ => {}
        }
    }

    draws
}

fn layer_scissor(
    bounds: Rect,
    scroll_y: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> (u32, u32, u32, u32) {
    let x0 = bounds.x.max(0.0).min(viewport_width);
    let y0 = (bounds.y - scroll_y).max(0.0).min(viewport_height);
    let x1 = (bounds.x + bounds.width).max(0.0).min(viewport_width);
    let y1 = (bounds.y - scroll_y + bounds.height)
        .max(0.0)
        .min(viewport_height);
    (
        x0.floor() as u32,
        y0.floor() as u32,
        (x1 - x0).max(0.0).ceil() as u32,
        (y1 - y0).max(0.0).ceil() as u32,
    )
}

fn solid_quad_vertices(
    rect: Rect,
    viewport_width: f32,
    viewport_height: f32,
    color: [f32; 4],
) -> [WebGlVertex; 6] {
    let left = rect.x / viewport_width * 2.0 - 1.0;
    let right = (rect.x + rect.width) / viewport_width * 2.0 - 1.0;
    let top = 1.0 - rect.y / viewport_height * 2.0;
    let bottom = 1.0 - (rect.y + rect.height) / viewport_height * 2.0;
    [
        WebGlVertex { position: [left, top], color },
        WebGlVertex { position: [right, top], color },
        WebGlVertex { position: [right, bottom], color },
        WebGlVertex { position: [left, top], color },
        WebGlVertex { position: [right, bottom], color },
        WebGlVertex { position: [left, bottom], color },
    ]
}

fn surface_rgba_bytes(surface: &Surface) -> Vec<u8> {
    let mut bytes =
        Vec::with_capacity(surface.pixels.len().saturating_mul(4));
    for Rgba(r, g, b, a) in &surface.pixels {
        bytes.extend_from_slice(&[*r, *g, *b, *a]);
    }
    bytes
}

fn write_texture_rgba(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    data: &[u8],
) {
    let unpadded = width.saturating_mul(4);
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = ((unpadded + align - 1) / align) * align;

    if padded == unpadded {
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(unpadded),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        return;
    }

    let mut padded_data = vec![0u8; padded as usize * height as usize];
    for row in 0..height as usize {
        let src = row * unpadded as usize;
        let dst = row * padded as usize;
        padded_data[dst..dst + unpadded as usize]
            .copy_from_slice(&data[src..src + unpadded as usize]);
    }

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &padded_data,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(padded),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

fn quad_vertices(
    rect: Rect,
    viewport_width: f32,
    viewport_height: f32,
    alpha: f32,
) -> [Vertex; 6] {
    quad_vertices_uv(
        rect,
        viewport_width,
        viewport_height,
        [0.0, 0.0, 1.0, 1.0],
        alpha,
    )
}

fn quad_vertices_uv(
    rect: Rect,
    viewport_width: f32,
    viewport_height: f32,
    uv: [f32; 4],
    alpha: f32,
) -> [Vertex; 6] {
    let left = rect.x / viewport_width * 2.0 - 1.0;
    let right = (rect.x + rect.width) / viewport_width * 2.0 - 1.0;
    let top = 1.0 - rect.y / viewport_height * 2.0;
    let bottom = 1.0 - (rect.y + rect.height) / viewport_height * 2.0;
    let [u0, v0, u1, v1] = uv;
    let a = alpha.clamp(0.0, 1.0);

    [
        Vertex { position: [left, top], uv: [u0, v0], alpha: a },
        Vertex { position: [right, top], uv: [u1, v0], alpha: a },
        Vertex { position: [right, bottom], uv: [u1, v1], alpha: a },
        Vertex { position: [left, top], uv: [u0, v0], alpha: a },
        Vertex { position: [right, bottom], uv: [u1, v1], alpha: a },
        Vertex { position: [left, bottom], uv: [u0, v1], alpha: a },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[test]
    fn compositor_tiles_clip_to_layer_bounds() {
        let bounds = Rect {
            x: 10.0,
            y: 20.0,
            width: 700.0,
            height: 600.0,
        };
        let first = compositor_tile_rect(bounds, 0, 0);
        let edge = compositor_tile_rect(bounds, 1, 1);
        assert_eq!(first.width, 512.0);
        assert_eq!(first.height, 512.0);
        assert_eq!(edge.width, 188.0);
        assert_eq!(edge.height, 88.0);
    }

    #[test]
    fn quad_vertices_support_cropped_uvs() {
        let v = quad_vertices_uv(
            Rect { x: 0.0, y: 0.0, width: 50.0, height: 50.0 },
            100.0,
            100.0,
            [0.25, 0.0, 0.75, 1.0],
            1.0,
        );
        assert_eq!(v[0].uv, [0.25, 0.0]);
        assert_eq!(v[2].uv, [0.75, 1.0]);
    }

    fn quad_vertices_map_pixels_to_ndc() {
        let v = quad_vertices(
            Rect { x: 0.0, y: 0.0, width: 100.0, height: 50.0 },
            200.0,
            100.0,
            0.5,
        );
        assert_eq!(v[0].position, [-1.0, 1.0]);
        assert_eq!(v[2].position, [0.0, 0.0]);
        assert!((v[0].alpha - 0.5).abs() < 0.001);
    }
}
