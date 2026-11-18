//! Shared Vello renderer, retained raster textures and premultiplied composition.
use std::{collections::HashMap, sync::Mutex};

#[cfg(test)]
use iced::Size;
use iced::{Rectangle, wgpu, widget::shader};
use vello::kurbo::Affine;

use crate::Geometry;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    scenes: Vec<u64>,
    logical_bounds: [u32; 4],
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Owner {
    #[cfg(test)]
    Batch(u64),
    Layer(u64, usize),
    Tile(u64, u64),
}

#[derive(Debug)]
pub(crate) struct Primitive {
    owner: Owner,
    layers: Vec<Geometry>,
    logical_bounds: Rectangle,
    retained: Option<(Rectangle, Rectangle)>,
}

impl Primitive {
    /// Creates a single-layer test instance with a shared geometry cache identity.
    #[cfg(test)]
    pub(crate) fn new(geometry: Geometry, logical_size: Size) -> Self {
        Self::with_bounds(geometry, Rectangle::with_size(logical_size))
    }
    /// Creates a test instance for just the visible logical canvas region.
    #[cfg(test)]
    pub(crate) fn with_bounds(geometry: Geometry, logical_bounds: Rectangle) -> Self {
        Self::layers(geometry.id, vec![geometry], logical_bounds)
    }
    /// Batches ordered layers into the persistent output owned by one canvas widget.
    #[cfg(test)]
    pub(crate) fn layers(owner: u64, layers: Vec<Geometry>, logical_bounds: Rectangle) -> Self {
        Self {
            owner: Owner::Batch(owner),
            layers,
            logical_bounds,
            retained: None,
        }
    }
    /// Gives each ordered layer independent output; retained regions use their immutable identity.
    pub(crate) fn layer(owner: u64, index: usize, geometry: Geometry, clip: Rectangle) -> Self {
        let retained = geometry.retained;
        Self {
            owner: if retained.is_some() {
                Owner::Tile(owner, geometry.id)
            } else {
                Owner::Layer(owner, index)
            },
            layers: vec![geometry],
            logical_bounds: clip,
            retained,
        }
    }
}

struct Surface {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    binding: wgpu::BindGroup,
    mapping: wgpu::Buffer,
    mapping_bytes: Vec<u8>,
    last_used: u64,
    key: Key,
    used: bool,
}

pub(crate) struct Pipeline {
    // Iced requires Sync pipelines. Preparation holds an exclusive reference.
    renderer: Mutex<vello::Renderer>,
    composite: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    surfaces: HashMap<Owner, Surface>,
    frame: u64,
    #[cfg(test)]
    allocations: usize,
    #[cfg(test)]
    rasters: usize,
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let renderer = vello::Renderer::new(
            device,
            vello::RendererOptions {
                antialiasing_support: vello::AaSupport::area_only(),
                ..Default::default()
            },
        )
        .expect("Vello initialization failed");
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Vello canvas texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let source = format!(
            "const LINEAR_TARGET: bool = {};\n{}",
            iced::advanced::graphics::color::GAMMA_CORRECTION,
            include_str!("composite.wgsl")
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Vello canvas composite"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Vello canvas composite"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let composite = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Vello canvas composite"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        Self {
            renderer: Mutex::new(renderer),
            composite,
            layout,
            surfaces: HashMap::new(),
            frame: 0,
            #[cfg(test)]
            allocations: 0,
            #[cfg(test)]
            rasters: 0,
        }
    }

    fn trim(&mut self) {
        const RETAINED_BYTES: u64 = 64 * 1024 * 1024;
        self.frame += 1;
        self.surfaces.retain(|owner, surface| {
            let used = std::mem::replace(&mut surface.used, false);
            used || matches!(owner, Owner::Tile(..))
                && self.frame.saturating_sub(surface.last_used) <= 120
        });
        let mut bytes: u64 = self
            .surfaces
            .values()
            .map(|s| u64::from(s.key.width) * u64::from(s.key.height) * 4)
            .sum();
        let mut old: Vec<_> = self
            .surfaces
            .iter()
            .filter(|(_, s)| s.last_used + 1 < self.frame)
            .map(|(id, s)| (*id, s.last_used))
            .collect();
        old.sort_by_key(|(_, frame)| *frame);
        for (id, _) in old {
            if bytes <= RETAINED_BYTES {
                break;
            }
            if let Some(s) = self.surfaces.remove(&id) {
                bytes -= u64::from(s.key.width) * u64::from(s.key.height) * 4;
            }
        }
    }
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        viewport: &shader::Viewport,
    ) {
        let scale = f64::from(viewport.scale_factor());
        let raster = self.retained.map_or(self.logical_bounds, |(r, _)| r);
        let content = self.retained.map_or(self.logical_bounds, |(_, c)| c);
        let draw = content
            .intersection(&self.logical_bounds)
            .unwrap_or(self.logical_bounds);
        let mapping: Vec<u8> = [
            (draw.x - raster.x) / raster.width,
            (draw.y - raster.y) / raster.height,
            draw.width / raster.width,
            draw.height / raster.height,
            (draw.x - self.logical_bounds.x) / self.logical_bounds.width,
            (draw.y - self.logical_bounds.y) / self.logical_bounds.height,
            draw.width / self.logical_bounds.width,
            draw.height / self.logical_bounds.height,
        ]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
        let key = Key {
            scenes: self.layers.iter().map(|layer| layer.id).collect(),
            logical_bounds: [
                raster.x.to_bits(),
                raster.y.to_bits(),
                raster.width.to_bits(),
                raster.height.to_bits(),
            ],
            width: (f64::from(raster.width) * scale).ceil().max(1.0) as u32,
            height: (f64::from(raster.height) * scale).ceil().max(1.0) as u32,
        };
        if let Some(surface) = pipeline.surfaces.get_mut(&self.owner) {
            surface.used = true;
            surface.last_used = pipeline.frame;
            if surface.mapping_bytes != mapping {
                queue.write_buffer(&surface.mapping, 0, &mapping);
                surface.mapping_bytes.clone_from(&mapping);
            }
            if surface.key == key {
                return;
            }
        }
        let resize = pipeline.surfaces.get(&self.owner).is_none_or(|surface| {
            surface.key.width != key.width || surface.key.height != key.height
        });
        if resize {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Vello canvas raster"),
                size: wgpu::Extent3d {
                    width: key.width,
                    height: key.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Vello crop mapping"),
                size: 32,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: true,
            });
            uniform
                .slice(..)
                .get_mapped_range_mut()
                .copy_from_slice(&mapping);
            uniform.unmap();
            let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Vello canvas raster"),
                layout: &pipeline.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            });
            pipeline.surfaces.insert(
                self.owner,
                Surface {
                    _texture: texture,
                    view,
                    binding,
                    mapping: uniform,
                    mapping_bytes: mapping,
                    last_used: pipeline.frame,
                    key: key.clone(),
                    used: true,
                },
            );
            #[cfg(test)]
            {
                pipeline.allocations += 1;
            }
        }
        let surface = pipeline
            .surfaces
            .get_mut(&self.owner)
            .expect("canvas output exists");
        let transform = Affine::scale_non_uniform(
            f64::from(key.width) / f64::from(raster.width),
            f64::from(key.height) / f64::from(raster.height),
        ) * Affine::translate((-f64::from(raster.x), -f64::from(raster.y)));
        let mut scene = vello::Scene::new();
        for layer in &self.layers {
            scene.append(&layer.scene, Some(transform));
        }
        pipeline
            .renderer
            .get_mut()
            .expect("Vello renderer poisoned")
            .render_to_texture(
                device,
                queue,
                &scene,
                &surface.view,
                &vello::RenderParams {
                    base_color: vello::peniko::Color::TRANSPARENT,
                    width: key.width,
                    height: key.height,
                    antialiasing_method: vello::AaConfig::Area,
                },
            )
            .expect("Vello canvas rasterization failed");
        surface.key = key;
        #[cfg(test)]
        {
            pipeline.rasters += 1;
        }
    }

    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        let surface = &pipeline.surfaces[&self.owner];
        pass.set_pipeline(&pipeline.composite);
        pass.set_bind_group(0, &surface.binding, &[]);
        pass.draw(0..6, 0..1);
        true
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
