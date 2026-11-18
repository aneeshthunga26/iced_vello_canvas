//! Backend-specific scene rasterization on the host's existing GPU device.
#[cfg(not(feature = "gpu"))]
use crate::scene::Scene;
use iced::{Size, wgpu};
use vello::kurbo::Affine;
#[cfg(not(feature = "gpu"))]
use vello::peniko;

/// Ordered scene layers and their physical output rectangle for one raster job.
pub(crate) struct Raster<'a> {
    pub(crate) layers: &'a [crate::Geometry],
    pub(crate) transform: Affine,
    pub(crate) view: &'a wgpu::TextureView,
    pub(crate) size: Size<u32>,
}

#[cfg(not(feature = "gpu"))]
pub(crate) struct Backend(vello::Renderer);
#[cfg(feature = "gpu")]
pub(crate) struct Backend {
    renderer: vello_gpu::Renderer,
    resources: vello_gpu::Resources,
    scene: vello_gpu::Scene,
    curves: crate::curves::CurveCache,
}
impl Backend {
    /// Initializes pipelines once for the selected backend and shared Iced device.
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        #[cfg(not(feature = "gpu"))]
        {
            let _ = queue;
            Self(
                vello::Renderer::new(
                    device,
                    vello::RendererOptions {
                        antialiasing_support: vello::AaSupport::area_only(),
                        ..Default::default()
                    },
                )
                .expect("Vello initialization failed"),
            )
        }
        #[cfg(feature = "gpu")]
        {
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
                curves: crate::curves::CurveCache::default(),
            }
        }
    }
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
        #[cfg(not(feature = "gpu"))]
        {
            let _ = encoder;
            let mut scene = Scene::new();
            for layer in layers {
                scene.append(&layer.scene, Some(transform));
            }
            self.0
                .render_to_texture(
                    device,
                    queue,
                    &scene,
                    view,
                    &vello::RenderParams {
                        base_color: peniko::Color::TRANSPARENT,
                        width,
                        height,
                        antialiasing_method: vello::AaConfig::Area,
                    },
                )
                .expect("Vello canvas rasterization failed");
        }
        #[cfg(feature = "gpu")]
        {
            let width = u16::try_from(width).expect("Vello GPU width exceeds u16");
            let height = u16::try_from(height).expect("Vello GPU height exceeds u16");
            self.scene.reset_and_resize(width, height);
            for layer in layers {
                layer.scene.replay(
                    &mut self.scene,
                    &mut self.resources,
                    &mut self.curves,
                    transform,
                );
            }
            self.renderer
                .render(
                    &self.scene,
                    &mut self.resources,
                    encoder,
                    vello_gpu::RenderTarget {
                        view,
                        depth_view: None,
                    },
                    &vello_gpu::TextureBindings::new(),
                    vello_gpu::RenderOptions::default(),
                )
                .expect("Vello GPU canvas rasterization failed");
            let _ = (device, queue);
        }
    }
    /// Releases staging buffers only after all recorded work has been submitted.
    pub(crate) fn after_submit(&mut self) {
        #[cfg(feature = "gpu")]
        {
            self.renderer.after_submit();
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
