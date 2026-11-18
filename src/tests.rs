//! Offscreen checks of the actual Vello primitive and its Iced composition pipeline.
use std::sync::mpsc;

use iced::{
    Color, Point, Rectangle, Size, Vector, wgpu,
    widget::shader::{self, Pipeline as _, Primitive as _},
};

use super::{Pipeline, Primitive};
use crate::{Cache, Frame, Path, Stroke, Text};

#[test]
fn cache_rebuilds_for_resize_and_clear() {
    let cache = Cache::new();
    let mut builds = 0;
    let mut draw = |_: &mut Frame| builds += 1;
    let first = cache.draw(&(), Size::new(40.0, 30.0), &mut draw);
    let second = cache.draw(&(), Size::new(40.0, 30.0), &mut draw);
    assert_eq!(first.id, second.id);
    let resized = cache.draw(&(), Size::new(41.0, 30.0), &mut draw);
    assert_ne!(first.id, resized.id);
    cache.clear();
    let cleared = cache.draw(&(), Size::new(41.0, 30.0), &mut draw);
    assert_ne!(resized.id, cleared.id);
    let shifted = cache.draw_with_bounds(
        &(),
        Rectangle::new(Point::new(4.0, 2.0), Size::new(41.0, 30.0)),
        &mut draw,
    );
    assert_ne!(cleared.id, shifted.id);
    assert_eq!(builds, 4);
}

#[test]
#[ignore = "requires a compute-capable GPU; run with --include-ignored"]
fn rebuilt_layers_reuse_the_raster_until_pixels_change() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("GPU adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_bind_groups: 2,
            max_non_sampler_bindings: 2048,
            ..Default::default()
        },
        ..Default::default()
    }))
    .expect("Iced-compatible device");
    let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    let bounds = Rectangle::with_size(Size::new(128.0, 128.0));
    let viewport = shader::Viewport::with_physical_size(Size::new(128, 128), 1.0);
    let overlay = |label: &str, color| {
        let mut frame = Frame::new(&(), bounds.size());
        frame.fill_rectangle(Point::new(8.0, 8.0), Size::new(16.0, 16.0), color);
        frame.fill_text(Text {
            content: label.into(),
            position: Point::new(28.0, 32.0),
            size: 22.0.into(),
            color: Color::WHITE,
            ..Default::default()
        });
        frame.into_geometry()
    };
    let first = overlay("21 cells", Color::WHITE);
    let repeated = overlay("21 cells", Color::WHITE);
    assert_ne!(first.id, repeated.id);
    let first = Primitive::layer(1, 0, first, bounds);
    first.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    let original = render(&device, &queue, &pipeline, &[&first], 128);
    let repeated = Primitive::layer(1, 0, repeated, bounds);
    repeated.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    assert_eq!(pipeline.rasters, 1);
    assert_eq!(
        render(&device, &queue, &pipeline, &[&repeated], 128),
        original
    );

    let updated = Primitive::layer(1, 0, overlay("22 cells", Color::WHITE), bounds);
    updated.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    assert_eq!(pipeline.rasters, 2);
    assert_ne!(
        render(&device, &queue, &pipeline, &[&updated], 128),
        original
    );

    let blue = Primitive::layer(
        1,
        0,
        overlay("22 cells", Color::from_rgb(0.0, 0.0, 1.0)),
        bounds,
    );
    blue.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    assert_eq!(pipeline.rasters, 3);
    assert_eq!(pipeline.allocations, 1);
    let pixels = render(&device, &queue, &pipeline, &[&blue], 128);
    assert_pixel(&pixels, 128, 12, 12, [0, 0, 255, 255]);

    let doubled = shader::Viewport::with_physical_size(Size::new(256, 256), 2.0);
    let resized = Primitive::layer(
        1,
        0,
        overlay("22 cells", Color::from_rgb(0.0, 0.0, 1.0)),
        bounds,
    );
    resized.prepare(&mut pipeline, &device, &queue, &bounds, &doubled);
    assert_eq!(pipeline.rasters, 4);
    assert_eq!(pipeline.allocations, 2);
}

#[test]
#[ignore = "requires a compute-capable GPU; run with --include-ignored"]
fn gpu_clip_alpha_text_multiple_scenes_and_dpi() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("This GPU test requires a compute-capable adapter");
    eprintln!("GPU adapter: {:?}", adapter.get_info());
    // Match Iced's native device request, including its reduced bind-group limit.
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_bind_groups: 2,
            max_non_sampler_bindings: 2048,
            ..Default::default()
        },
        ..Default::default()
    }))
    .expect("Iced-compatible device");
    let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    let mut frame = Frame::new(&(), Size::new(128.0, 128.0));
    frame.fill_rectangle(Point::ORIGIN, frame.size(), Color::from_rgb(0.0, 0.0, 1.0));
    frame.with_save(|frame| {
        frame.translate(Vector::new(8.0, 8.0));
        frame.with_clip(
            Rectangle::new(Point::new(8.0, 8.0), Size::new(16.0, 16.0)),
            |frame| {
                frame.fill_rectangle(
                    Point::new(-10.0, -10.0),
                    Size::new(50.0, 50.0),
                    Color::from_rgba(1.0, 0.0, 0.0, 0.5),
                );
            },
        );
    });
    frame.fill_rectangle(
        Point::new(32.0, 0.0),
        Size::new(16.0, 16.0),
        Color::from_rgb(0.5, 0.5, 0.5),
    );
    frame.fill_rectangle(Point::new(0.0, 48.0), Size::new(128.0, 40.0), Color::WHITE);
    frame.fill_text(Text {
        content: "ffi café".into(),
        position: Point::new(2.0, 50.0),
        size: 24.0.into(),
        shaping: iced::advanced::text::Shaping::Advanced,
        ..Default::default()
    });
    let geometry = frame.into_geometry();
    let first = Primitive::new(geometry.clone(), geometry.size);
    let bounds = Rectangle::with_size(geometry.size);
    let viewport = shader::Viewport::with_physical_size(Size::new(128, 128), 1.0);
    first.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    assert_eq!(pipeline.surfaces.len(), 1);
    let pixels = render(&device, &queue, &pipeline, &[&first], 128);
    assert_pixel(&pixels, 128, 4, 4, [0, 0, 255, 255]);
    let half = (iced::advanced::graphics::color::pack(Color::from_rgb(0.5, 0.5, 0.5)).components()
        [0]
        * 255.0)
        .round() as u8;
    assert_pixel(&pixels, 128, 12, 12, [half, 0, half, 255]);
    assert_pixel(&pixels, 128, 25, 25, [0, 0, 255, 255]);
    assert_pixel(&pixels, 128, 4, 32, [0, 0, 255, 255]); // transform restored
    let gray = (iced::advanced::graphics::color::pack(Color::from_rgb(0.5, 0.5, 0.5)).components()
        [0]
        * 255.0)
        .round() as u8;
    assert_pixel(&pixels, 128, 36, 4, [gray, gray, gray, 255]);
    let ink = (50..82)
        .flat_map(|y| (2..100).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(&pixels, 128, x, y)[0] < 200)
        .count();
    assert!(
        ink > 150,
        "Vello must actually rasterize shaped glyphs: {ink}"
    );
    let same = Primitive::new(geometry, Size::new(128.0, 128.0));
    same.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    assert_eq!(
        pipeline.surfaces.len(),
        1,
        "unchanged scene must reuse its raster"
    );

    // A second simultaneous canvas must keep its own raster, with analytic edge coverage.
    let mut frame = Frame::new(&(), Size::new(128.0, 128.0));
    frame.fill(&Path::circle(Point::new(32.3, 32.3), 10.0), Color::WHITE);
    let second = Primitive::new(frame.into_geometry(), Size::new(128.0, 128.0));
    second.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    assert_eq!(pipeline.surfaces.len(), 2);
    let circle = render(&device, &queue, &pipeline, &[&second], 128);
    assert_pixel(&circle, 128, 32, 32, [255, 255, 255, 255]);
    assert_pixel(&circle, 128, 0, 0, [0, 0, 0, 0]);
    assert!(
        circle
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0 && p[3] < 255),
        "curves need fractional AA coverage"
    );
    assert_eq!(
        render(&device, &queue, &pipeline, &[&first], 128),
        pixels,
        "another canvas must not overwrite the first"
    );
    let viewport = shader::Viewport::with_physical_size(Size::new(256, 256), 2.0);
    same.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    assert_eq!(
        pipeline.surfaces.len(),
        2,
        "DPI changes replace the canvas output"
    );
    assert_eq!(pipeline.allocations, 3);
    let doubled = render(&device, &queue, &pipeline, &[&same], 256);
    assert_pixel(&doubled, 256, 24, 24, [half, 0, half, 255]);
    let mut bounded = Frame::with_bounds(
        &(),
        Rectangle::new(Point::new(60.0, 100.0), Size::new(16.0, 16.0)),
    );
    bounded.fill_rectangle(
        Point::ORIGIN,
        Size::new(128.0, 128.0),
        Color::from_rgb(1.0, 0.0, 0.0),
    );
    let bounded = Primitive::new(bounded.into_geometry(), Size::new(128.0, 128.0));
    bounded.prepare(
        &mut pipeline,
        &device,
        &queue,
        &bounds,
        &shader::Viewport::with_physical_size(Size::new(128, 128), 1.0),
    );
    let clipped = render(&device, &queue, &pipeline, &[&bounded], 128);
    assert_pixel(&clipped, 128, 65, 105, [255, 0, 0, 255]);
    assert_pixel(&clipped, 128, 59, 105, [0, 0, 0, 0]);

    let mut scaled = Frame::new(&(), Size::new(128.0, 128.0));
    scaled.scale(3.0_f32);
    scaled.stroke(
        &Path::line(Point::new(3.0, 20.0), Point::new(30.0, 20.0)),
        Stroke::default().with_color(Color::WHITE).with_width(2.0),
    );
    let scaled = Primitive::new(scaled.into_geometry(), Size::new(128.0, 128.0));
    scaled.prepare(
        &mut pipeline,
        &device,
        &queue,
        &bounds,
        &shader::Viewport::with_physical_size(Size::new(128, 128), 1.0),
    );
    let scaled_pixels = render(&device, &queue, &pipeline, &[&scaled], 128);
    assert_pixel(&scaled_pixels, 128, 30, 59, [255, 255, 255, 255]);
    assert_pixel(&scaled_pixels, 128, 30, 60, [255, 255, 255, 255]);
    assert_pixel(&scaled_pixels, 128, 30, 57, [0, 0, 0, 0]);

    // A large drawing is far wider than the device texture limit. Only the
    // exposed rectangle may be rasterized, retaining canvas-space coordinates.
    let mut long = Frame::new(&(), Size::new(100_000.0, 128.0));
    long.fill_rectangle(
        Point::new(50_000.0, 0.0),
        Size::new(20.0, 128.0),
        Color::from_rgb(1.0, 0.0, 0.0),
    );
    let long = long.into_geometry();
    let clipped_drawing = Primitive::with_bounds(
        long.clone(),
        Rectangle::new(Point::new(49_990.0, 0.0), Size::new(128.0, 128.0)),
    );
    clipped_drawing.prepare(
        &mut pipeline,
        &device,
        &queue,
        &bounds,
        &shader::Viewport::with_physical_size(Size::new(128, 128), 1.0),
    );
    let cropped_pixels = render(&device, &queue, &pipeline, &[&clipped_drawing], 128);
    assert_pixel(&cropped_pixels, 128, 12, 10, [255, 0, 0, 255]);
    assert_pixel(&cropped_pixels, 128, 2, 10, [0, 0, 0, 0]);
    let shifted_drawing = Primitive::with_bounds(
        long,
        Rectangle::new(Point::new(50_010.0, 0.0), Size::new(128.0, 128.0)),
    );
    shifted_drawing.prepare(
        &mut pipeline,
        &device,
        &queue,
        &bounds,
        &shader::Viewport::with_physical_size(Size::new(128, 128), 1.0),
    );
    let shifted_pixels = render(&device, &queue, &pipeline, &[&shifted_drawing], 128);
    assert_pixel(&shifted_pixels, 128, 2, 10, [255, 0, 0, 255]);
    assert_pixel(&shifted_pixels, 128, 12, 10, [0, 0, 0, 0]);

    let mut overlay = Frame::new(&(), Size::new(128.0, 128.0));
    overlay.fill_rectangle(
        Point::ORIGIN,
        Size::new(128.0, 128.0),
        Color::from_rgba(1.0, 0.0, 0.0, 0.5),
    );
    let overlay = Primitive::new(overlay.into_geometry(), Size::new(128.0, 128.0));
    overlay.prepare(
        &mut pipeline,
        &device,
        &queue,
        &bounds,
        &shader::Viewport::with_physical_size(Size::new(128, 128), 1.0),
    );
    first.prepare(
        &mut pipeline,
        &device,
        &queue,
        &bounds,
        &shader::Viewport::with_physical_size(Size::new(128, 128), 1.0),
    );
    let composited = render(&device, &queue, &pipeline, &[&first, &overlay], 128);
    assert_pixel(&composited, 128, 4, 4, [128, 0, 127, 255]);
    // The retained GPU frontend must transform paint with its path and preserve holes.
    let mut painted = Frame::new(&(), Size::new(128.0, 128.0));
    painted.translate(Vector::new(32.0, 16.0));
    let gradient = crate::gradient::Linear::new(Point::ORIGIN, Point::new(64.0, 0.0))
        .add_stop(0.0, Color::from_rgb(1.0, 0.0, 0.0))
        .add_stop(1.0, Color::from_rgb(0.0, 0.0, 1.0));
    painted.fill_rectangle(Point::ORIGIN, Size::new(64.0, 16.0), gradient);
    let ring = Path::new(|builder| {
        builder.rectangle(Point::new(0.0, 32.0), Size::new(64.0, 64.0));
        builder.rectangle(Point::new(16.0, 48.0), Size::new(32.0, 32.0));
    });
    painted.fill(
        &ring,
        crate::Fill {
            style: Color::WHITE.into(),
            rule: crate::fill::Rule::EvenOdd,
        },
    );
    let painted = Primitive::new(painted.into_geometry(), Size::new(128.0, 128.0));
    painted.prepare(
        &mut pipeline,
        &device,
        &queue,
        &bounds,
        &shader::Viewport::with_physical_size(Size::new(128, 128), 1.0),
    );
    let paints = render(&device, &queue, &pipeline, &[&painted], 128);
    let red = pixel(&paints, 128, 36, 20);
    let blue = pixel(&paints, 128, 90, 20);
    assert!(
        red[0] > red[2] && blue[2] > blue[0],
        "gradient must follow the translated path"
    );
    assert_pixel(&paints, 128, 36, 52, [255, 255, 255, 255]);
    assert_pixel(&paints, 128, 60, 80, [0, 0, 0, 0]);

    // Layer batching, changed content and changed crop must reuse one canvas allocation.
    let owner = u64::MAX;
    let make = |color| {
        let mut f = Frame::new(&(), Size::new(128.0, 128.0));
        f.fill_rectangle(Point::ORIGIN, f.size(), color);
        f.into_geometry()
    };
    let base = make(Color::from_rgb(0.0, 0.0, 1.0));
    let top = make(Color::from_rgba(1.0, 0.0, 0.0, 0.5));
    let batch = Primitive::layers(owner, vec![base.clone(), top.clone()], bounds);
    let viewport = shader::Viewport::with_physical_size(Size::new(128, 128), 1.0);
    let before = (pipeline.allocations, pipeline.rasters);
    batch.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    assert_eq!(
        (pipeline.allocations, pipeline.rasters),
        (before.0 + 1, before.1 + 1)
    );
    assert_pixel(
        &render(&device, &queue, &pipeline, &[&batch], 128),
        128,
        4,
        4,
        [half, 0, half, 255],
    );
    batch.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    assert_eq!(
        pipeline.rasters,
        before.1 + 1,
        "unchanged layers must skip rasterization"
    );
    let changed = Primitive::layers(owner, vec![make(Color::from_rgb(0.0, 1.0, 0.0))], bounds);
    changed.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    assert_eq!(
        pipeline.allocations,
        before.0 + 1,
        "changed scenes reuse output storage"
    );
    assert_pixel(
        &render(&device, &queue, &pipeline, &[&changed], 128),
        128,
        4,
        4,
        [0, 255, 0, 255],
    );
    let shifted = Primitive::layers(owner, vec![base, top], Rectangle { x: 10.0, ..bounds });
    shifted.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    assert_eq!(
        pipeline.allocations,
        before.0 + 1,
        "scrolling reuses output storage"
    );
    assert_pixel(
        &render(&device, &queue, &pipeline, &[&shifted], 128),
        128,
        120,
        4,
        [0, 0, 0, 0],
    );
    // Identical straight strokes must keep caps, alpha, gradients and logical widths
    // when routed through rectangles. A collinear cubic forces the native path fallback.
    let stroke_scene = |curved: bool| {
        let mut frame = Frame::new(&(), Size::new(128.0, 128.0));
        frame.translate(Vector::new(4.25, 2.5));
        frame.scale(1.5_f32);
        for (i, cap) in [
            crate::LineCap::Butt,
            crate::LineCap::Square,
            crate::LineCap::Round,
        ]
        .into_iter()
        .enumerate()
        {
            let y = 10.25 + i as f32 * 14.0;
            let path = if curved {
                Path::new(|builder| {
                    builder.move_to(Point::new(10.0, y));
                    builder.bezier_curve_to(
                        Point::new(25.0, y),
                        Point::new(45.0, y),
                        Point::new(60.0, y),
                    );
                })
            } else {
                Path::line(Point::new(10.0, y), Point::new(60.0, y))
            };
            let gradient = crate::gradient::Linear::new(Point::new(10.0, y), Point::new(60.0, y))
                .add_stop(0.0, Color::from_rgba(1.0, 0.0, 0.0, 0.6))
                .add_stop(1.0, Color::from_rgba(0.0, 0.0, 1.0, 0.6));
            frame.stroke(
                &path,
                Stroke {
                    line_cap: cap,
                    style: crate::Style::Gradient(gradient.into()),
                    ..Stroke::default()
                        .with_width(1.5 + i as f32)
                        .with_color(Color::WHITE)
                },
            );
        }
        let path = if curved {
            Path::new(|builder| {
                builder.move_to(Point::new(66.0, 50.0));
                builder.bezier_curve_to(
                    Point::new(66.0, 40.0),
                    Point::new(66.0, 20.0),
                    Point::new(66.0, 10.0),
                );
            })
        } else {
            Path::line(Point::new(66.0, 50.0), Point::new(66.0, 10.0))
        };
        frame.stroke(
            &path,
            Stroke::default()
                .with_color(Color::from_rgba(0.2, 0.4, 0.8, 0.7))
                .with_width(1.3)
                .with_line_cap(crate::LineCap::Square),
        );
        Primitive::new(frame.into_geometry(), Size::new(128.0, 128.0))
    };
    let line = stroke_scene(false);
    let curve = stroke_scene(true);
    line.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    curve.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    let line_pixels = render(&device, &queue, &pipeline, &[&line], 128);
    let curve_pixels = render(&device, &queue, &pipeline, &[&curve], 128);
    assert!(
        line_pixels
            .iter()
            .zip(&curve_pixels)
            .all(|(a, b)| a.abs_diff(*b) <= 3),
        "stroke rectangle optimization must preserve native-path rendering"
    );

    // Reused text must retain position and colour across warm atlas draws, clips,
    // scaling and the arbitrary-transform vector fallback.
    let text_scene = |color, shift: f32, rotated, poison_rule| {
        let mut f = Frame::new(&(), Size::new(128.0, 128.0));
        f.with_clip(
            Rectangle::new(Point::new(4.0, 4.0), Size::new(110.0, 110.0)),
            |f| {
                f.translate(Vector::new(shift, 0.0));
                if poison_rule {
                    f.fill(
                        &Path::rectangle(Point::new(300.0, 300.0), Size::new(20.0, 20.0)),
                        crate::Fill {
                            style: Color::TRANSPARENT.into(),
                            rule: crate::fill::Rule::EvenOdd,
                        },
                    );
                }
                if rotated {
                    f.rotate(0.12_f32);
                }
                f.fill_text(Text {
                    content: "Mfi".into(),
                    position: Point::new(8.0, 16.0),
                    size: 28.0.into(),
                    color,
                    ..Text::default()
                });
            },
        );
        Primitive::new(f.into_geometry(), Size::new(128.0, 128.0))
    };
    let red_text = text_scene(Color::from_rgb(1.0, 0.0, 0.0), 0.0, false, false);
    red_text.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    let red_text_pixels = render(&device, &queue, &pipeline, &[&red_text], 128);
    let red_ink = red_text_pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] > 100 && p[2] < 20 && p[3] > 100)
        .count();
    assert!(red_ink > 100);
    let blue_text = text_scene(Color::from_rgb(0.0, 0.0, 1.0), 16.0, false, false);
    blue_text.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    let blue_text_pixels = render(&device, &queue, &pipeline, &[&blue_text], 128);
    for y in 8..65 {
        for x in 8..90 {
            let red = pixel(&red_text_pixels, 128, x, y);
            let blue = pixel(&blue_text_pixels, 128, x + 16, y);
            assert!(
                red[3].abs_diff(blue[3]) <= 3,
                "cached glyph coverage must follow position"
            );
            assert!(blue[0] <= 3, "cached glyph colour must follow paint");
        }
    }
    let rotated_text = text_scene(Color::WHITE, 0.0, true, false);
    rotated_text.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    let rotated_pixels = render(&device, &queue, &pipeline, &[&rotated_text], 128);
    assert!(
        rotated_pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 100)
            .count()
            > 100
    );

    let rule_text = text_scene(Color::WHITE, 0.0, true, true);
    rule_text.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
    let rule_pixels = render(&device, &queue, &pipeline, &[&rule_text], 128);
    assert!(
        rotated_pixels
            .iter()
            .zip(&rule_pixels)
            .all(|(a, b)| a.abs_diff(*b) <= 3),
        "text winding must not inherit a preceding path's even-odd rule"
    );

    pipeline.trim();
    pipeline.trim();
    assert!(
        pipeline.surfaces.is_empty(),
        "unused GPU rasters must be released"
    );
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &Pipeline,
    primitives: &[&Primitive],
    size: u32,
) -> Vec<u8> {
    let mut encoder = device.create_command_encoder(&Default::default());
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("canvas test target"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let row_bytes = (size * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("canvas test readback"),
        size: u64::from(row_bytes * size),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_viewport(0.0, 0.0, size as f32, size as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, size, size);
        for primitive in primitives {
            assert!(primitive.draw(pipeline, &mut pass));
        }
    }
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(size),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    buffer
        .slice(..)
        .get_mapped_range()
        .chunks_exact(row_bytes as usize)
        .flat_map(|row| row[..(size * 4) as usize].iter().copied())
        .collect()
}

fn pixel(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    pixels[(y * width + x) * 4..][..4].try_into().unwrap()
}

fn assert_pixel(pixels: &[u8], width: usize, x: usize, y: usize, expected: [u8; 4]) {
    let actual = pixel(pixels, width, x, y);
    assert!(
        actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
        "pixel ({x}, {y}): {actual:?}, expected {expected:?}"
    );
}

#[ignore = "requires an actual GPU"]
#[test]
fn retained_tiles_preserve_glyphs_and_reuse_rasters() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_bind_groups: 2,
            max_non_sampler_bindings: 2048,
            ..Default::default()
        },
        ..Default::default()
    }))
    .unwrap();
    let mut pipeline = Pipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    let viewport = shader::Viewport::with_physical_size(Size::new(128, 128), 1.0);
    let clip = Rectangle::with_size(Size::new(128., 128.));
    let tile = |left: f32, content: &str| {
        let core = Rectangle::new(Point::new(left, 0.), Size::new(64., 128.));
        let raster = Rectangle {
            x: left - 2.,
            y: -2.,
            width: 68.,
            height: 132.,
        };
        let mut frame = Frame::with_bounds(&(), raster);
        frame.fill_rectangle(Point::new(-2., -2.), Size::new(140., 140.), Color::WHITE);
        frame.fill(
            &Path::circle(Point::new(64., 75.), 20.),
            Color::from_rgba(1., 0., 0., 0.5),
        );
        frame.fill_text(Text {
            content: content.into(),
            position: Point::new(left + 5., 10.),
            size: 24.0.into(),
            color: Color::BLACK,
            ..Default::default()
        });
        frame.into_geometry().retained_region(raster, core)
    };
    let a = tile(0., "A");
    let b = tile(64., "Ωfi");
    let first = Primitive::layer(100, 0, a.clone(), clip);
    let second = Primitive::layer(100, 1, b.clone(), clip);
    for primitive in [&first, &second] {
        primitive.prepare(&mut pipeline, &device, &queue, &clip, &viewport);
    }
    let pixels = render(&device, &queue, &pipeline, &[&first, &second], 128);
    let half = (iced::advanced::graphics::color::pack(Color::from_rgb(0.5, 0.5, 0.5)).components()
        [0]
        * 255.)
        .round() as u8;
    for x in [62, 63, 64, 65] {
        assert_pixel(&pixels, 128, x, 75, [255, half, half, 255]);
    }
    for (lo, hi) in [(2, 50), (68, 125)] {
        assert!(
            (10..40)
                .flat_map(|y| (lo..hi).map(move |x| (x, y)))
                .filter(|&(x, y)| pixel(&pixels, 128, x, y)[0] < 100)
                .count()
                > 30,
            "cold glyph ink missing"
        );
    }
    let rasters = pipeline.rasters;
    let allocations = pipeline.allocations;
    // Fractional horizontal pan uses existing tile pixels; the seam stays opaque.
    let pan = Rectangle { x: 0.375, ..clip };
    let first = Primitive::layer(100, 0, a.clone(), pan);
    let second = Primitive::layer(100, 1, b.clone(), pan);
    for primitive in [&first, &second] {
        primitive.prepare(&mut pipeline, &device, &queue, &clip, &viewport);
    }
    let panned = render(&device, &queue, &pipeline, &[&first, &second], 128);
    assert_eq!(pipeline.rasters, rasters);
    assert_eq!(pipeline.allocations, allocations);
    for x in [62, 63, 64, 65] {
        assert_pixel(&panned, 128, x, 75, [255, half, half, 255]);
    }
    // A transient overlay has a separate surface and does not rerasterize the background.
    let mut overlay = Frame::new(&(), clip.size());
    overlay.fill_rectangle(
        Point::new(60., 70.),
        Size::new(12., 12.),
        Color::from_rgb(0., 0., 1.),
    );
    let overlay = Primitive::layer(100, 2, overlay.into_geometry(), pan);
    for primitive in [&first, &second, &overlay] {
        primitive.prepare(&mut pipeline, &device, &queue, &clip, &viewport);
    }
    let selected = render(
        &device,
        &queue,
        &pipeline,
        &[&first, &second, &overlay],
        128,
    );
    assert_eq!(pipeline.rasters, rasters + 1);
    assert_pixel(&selected, 128, 64, 75, [0, 0, 255, 255]);
    // Different scale must replace tile pixels, while unused retention eventually expires.
    let doubled = shader::Viewport::with_physical_size(Size::new(256, 256), 2.0);
    first.prepare(&mut pipeline, &device, &queue, &clip, &doubled);
    assert_eq!(pipeline.rasters, rasters + 2);
    for _ in 0..122 {
        pipeline.trim();
    }
    assert!(pipeline.surfaces.is_empty());
    first.prepare(&mut pipeline, &device, &queue, &clip, &viewport);
    let restored = render(&device, &queue, &pipeline, &[&first], 128);
    assert_pixel(&restored, 128, 25, 110, [255, 255, 255, 255]);

    // Pressure evicts cold outputs while protecting this frame's visible region.
    let large = Rectangle::with_size(Size::new(2048., 2048.));
    let regions: Vec<_> = (0..5)
        .map(|index| {
            let mut frame = Frame::new(&(), large.size());
            frame.fill_rectangle(Point::ORIGIN, large.size(), Color::WHITE);
            Primitive::layer(
                200,
                index,
                frame.into_geometry().retained_region(large, large),
                large,
            )
        })
        .collect();
    for region in &regions {
        region.prepare(&mut pipeline, &device, &queue, &large, &viewport);
    }
    pipeline.trim();
    assert!(
        regions
            .iter()
            .all(|r| pipeline.surfaces.contains_key(&r.owner))
    );
    regions[4].prepare(&mut pipeline, &device, &queue, &large, &viewport);
    pipeline.trim();
    let bytes: u64 = pipeline
        .surfaces
        .values()
        .map(|s| u64::from(s.key.width) * u64::from(s.key.height) * 4)
        .sum();
    assert!(bytes <= 64 * 1024 * 1024);
    assert!(pipeline.surfaces.contains_key(&regions[4].owner));
    let evicted = regions
        .iter()
        .find(|r| !pipeline.surfaces.contains_key(&r.owner))
        .unwrap();
    let allocations = pipeline.allocations;
    evicted.prepare(&mut pipeline, &device, &queue, &large, &viewport);
    assert_eq!(pipeline.allocations, allocations + 1);
    let restored = render(&device, &queue, &pipeline, &[evicted], 128);
    assert_pixel(&restored, 128, 25, 110, [255, 255, 255, 255]);
}
