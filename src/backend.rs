//! Backend-specific scene rasterization on the host's existing GPU device.
use crate::{RendererBackend, scene::Scene};
use iced::{Size, wgpu};
use vello::kurbo::Affine;

/// Ordered scene layers and their physical output rectangle for one raster job.
pub(crate) struct Raster<'a> {
    pub(crate) layers: &'a [crate::Geometry],
    pub(crate) transform: Affine,
    pub(crate) view: &'a wgpu::TextureView,
    pub(crate) size: Size<u32>,
}

/// Lazily initialized rasterizers sharing Iced's device and queue.
#[derive(Default)]
pub(crate) struct Backend {
    #[cfg(feature = "gpu")]
    strips: Option<StripRenderer>,
    #[cfg(feature = "gpu")]
    curves: crate::curves::CurveCache,
    #[cfg(any(feature = "compute", not(feature = "gpu")))]
    compute: Option<vello::Renderer>,
}

#[cfg(feature = "gpu")]
struct StripRenderer {
    renderer: vello_gpu::Renderer,
    resources: vello_gpu::Resources,
    scene: vello_gpu::Scene,
}

#[cfg(feature = "gpu")]
impl StripRenderer {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let (renderer, resources) = vello_gpu::Renderer::new(
            device,
            queue,
            &vello_gpu::RenderTargetConfig {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 1,
                height: 1,
            },
        );
        Self {
            renderer,
            resources,
            scene: vello_gpu::Scene::new(1, 1),
        }
    }
}

impl Backend {
    /// Renders one combined canvas job, preserving retained font/backend resource caches.
    #[cfg(test)]
    #[allow(dead_code)] // Standalone benchmark driver compiles these production modules.
    pub(crate) fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layers: &[crate::Geometry],
        transform: Affine,
        view: &wgpu::TextureView,
        size: Size<u32>,
    ) {
        let mut encoder = device.create_command_encoder(&Default::default());
        self.record(
            device,
            queue,
            &mut encoder,
            Raster {
                layers,
                transform,
                view,
                size,
            },
        );
        queue.submit([encoder.finish()]);
        self.after_submit();
    }

    /// Records strip rendering in the host encoder; compute Vello retains its own submission API.
    pub(crate) fn record(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        raster: Raster<'_>,
    ) {
        let Raster {
            layers,
            transform,
            view,
            size: Size { width, height },
        } = raster;
        let backend = layers
            .first()
            .map_or_else(RendererBackend::default, |layer| layer.scene.backend());
        match backend {
            #[cfg(any(feature = "compute", not(feature = "gpu")))]
            RendererBackend::Compute => {
                let _ = encoder;
                let renderer = self.compute.get_or_insert_with(|| {
                    vello::Renderer::new(
                        device,
                        vello::RendererOptions {
                            antialiasing_support: vello::AaSupport::area_only(),
                            ..Default::default()
                        },
                    )
                    .expect("Vello compute initialization failed")
                });
                let mut scene = vello::Scene::new();
                for layer in layers {
                    #[cfg(all(feature = "compute", feature = "gpu"))]
                    let Scene::Compute(encoded) = &*layer.scene else {
                        unreachable!("one raster job uses one Vello backend");
                    };
                    #[cfg(not(feature = "gpu"))]
                    let Scene::Compute(encoded) = &*layer.scene;
                    scene.append(encoded, Some(transform));
                }
                renderer
                    .render_to_texture(
                        device,
                        queue,
                        &scene,
                        view,
                        &vello::RenderParams {
                            base_color: vello::peniko::Color::TRANSPARENT,
                            width,
                            height,
                            antialiasing_method: vello::AaConfig::Area,
                        },
                    )
                    .expect("Vello compute canvas rasterization failed");
            }
            #[cfg(feature = "gpu")]
            RendererBackend::Gpu => {
                let strips = self
                    .strips
                    .get_or_insert_with(|| StripRenderer::new(device, queue));
                let width = u16::try_from(width).expect("Vello GPU width exceeds u16");
                let height = u16::try_from(height).expect("Vello GPU height exceeds u16");
                strips.scene.reset_and_resize(width, height);
                for layer in layers {
                    #[cfg(feature = "compute")]
                    let Scene::Gpu(encoded) = &*layer.scene else {
                        unreachable!("one raster job uses one Vello backend");
                    };
                    #[cfg(not(feature = "compute"))]
                    let Scene::Gpu(encoded) = &*layer.scene;
                    encoded.replay(
                        &mut strips.scene,
                        &mut strips.resources,
                        &mut self.curves,
                        transform,
                    );
                }
                strips
                    .renderer
                    .render(
                        &strips.scene,
                        &mut strips.resources,
                        encoder,
                        vello_gpu::RenderTarget {
                            view,
                            depth_view: None,
                        },
                        &vello_gpu::TextureBindings::new(),
                        vello_gpu::RenderOptions::default(),
                    )
                    .expect("Vello GPU canvas rasterization failed");
            }
        }
    }

    /// Releases staging buffers only after all recorded work has been submitted.
    pub(crate) fn after_submit(&mut self) {
        #[cfg(feature = "gpu")]
        if let Some(strips) = &mut self.strips {
            strips.renderer.after_submit();
            self.curves.end_frame();
        }
    }
}

#[cfg(all(test, feature = "gpu"))]
impl Backend {
    /// Selects native curves for actual-pixel reference comparisons.
    pub(crate) fn set_curve_cache_enabled(&mut self, enabled: bool) {
        self.curves.set_enabled(enabled);
    }
    /// Returns hit/preparation counters and retained entry/byte budgets.
    pub(crate) fn curve_stats(&self) -> (u64, u64, usize, usize) {
        self.curves.stats()
    }
}
