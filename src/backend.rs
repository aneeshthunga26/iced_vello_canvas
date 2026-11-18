//! Compute rasterization on Iced's existing GPU device.
use iced::{Size, wgpu};
use vello::kurbo::Affine;

/// Reuses one Vello renderer for a pipeline's raster jobs.
pub(crate) struct Backend(vello::Renderer);

impl Backend {
    /// Initializes the area-antialiasing compute pipeline for the host device.
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self(
            vello::Renderer::new(
                device,
                vello::RendererOptions {
                    antialiasing_support: vello::AaSupport::area_only(),
                    ..Default::default()
                },
            )
            .expect("Vello compute initialization failed"),
        )
    }

    /// Rasterizes ordered geometry into the supplied texture on the host queue.
    pub(crate) fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layers: &[crate::Geometry],
        transform: Affine,
        view: &wgpu::TextureView,
        size: Size<u32>,
    ) {
        let mut scene = vello::Scene::new();
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
                    base_color: vello::peniko::Color::TRANSPARENT,
                    width: size.width,
                    height: size.height,
                    antialiasing_method: vello::AaConfig::Area,
                },
            )
            .expect("Vello compute canvas rasterization failed");
    }
}
