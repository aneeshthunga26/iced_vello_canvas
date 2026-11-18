//! Manual real-GPU benchmark of filled/stroked curve preparation across changing views.
//! Arguments are timed frames (default 48) and an optional positive maximum FPS.
// Compile the same production modules into this private executable, so no benchmark API is
// added to the canvas crate. Unused consumer methods are intentional in this focused harness.
#![allow(dead_code)]
#[path = "../src/backend.rs"]
mod backend;
#[path = "../src/curves.rs"]
mod curves;
#[path = "../src/frame.rs"]
mod frame;
#[path = "../src/scene.rs"]
mod scene;
// Cargo sets cfg(test) for this harness-free bench; shared unit-test imports are unused here.
#[allow(unused_imports)]
#[path = "../src/text.rs"]
mod text;
pub use frame::{Frame, Geometry};
pub use iced::widget::canvas::{
    Fill, Gradient, LineCap, LineJoin, Path, Stroke, Style, Text, fill, path,
};
use iced::{Color, Point, Size, wgpu};
use std::{
    fs,
    time::{Duration, Instant},
};
use vello::kurbo::Affine;

fn fixture(count: usize, stroked: bool) -> Geometry {
    let mut frame = Frame::new(&(), Size::new(100_000.0, 768.0));
    for i in 0..count {
        let x = (i % 32) as f32 * 30.0 + 20.0;
        let y = (i / 32) as f32 * 30.0 + 20.0;
        // Vary curvature without making every path a unique translated copy.
        let bend = (i % 17) as f32 * 0.125;
        let path = Path::new(|p| {
            p.move_to(Point::new(x, y));
            p.bezier_curve_to(
                Point::new(x + 8.0, y - 12.0 - bend),
                Point::new(x + 13.0, y + 28.0),
                Point::new(x + 24.0, y + 10.0),
            );
            p.quadratic_curve_to(
                Point::new(x + 32.0, y + 27.0 + bend),
                Point::new(x + 10.0, y + 22.0),
            );
            if !stroked {
                p.close();
            }
        });
        if stroked {
            frame.stroke(
                &path,
                Stroke::default()
                    .with_width(2.0)
                    .with_line_cap(LineCap::Round)
                    .with_line_join(LineJoin::Round)
                    .with_color(Color::from_rgba(0.2, 0.5, 0.9, 0.8)),
            );
        } else {
            frame.fill(&path, Color::from_rgba(0.2, 0.5, 0.9, 0.8));
        }
    }
    frame.into_geometry()
}
fn cpu_ticks() -> Option<u64> {
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let fields: Vec<_> = stat.split_once(") ")?.1.split_whitespace().collect();
    Some(fields[11].parse::<u64>().ok()? + fields[12].parse::<u64>().ok()?)
}
// The limiter runs after the GPU wait, outside the scene CPU-stage timer.
fn pace(start: Instant, max_fps: Option<u32>) {
    if let Some(fps) = max_fps {
        let remaining =
            Duration::from_secs_f64(1.0 / f64::from(fps)).saturating_sub(start.elapsed());
        if !remaining.is_zero() {
            std::thread::sleep(remaining);
        }
    }
}
fn main() {
    // Cargo supplies --bench even for harness-free benchmarks.
    let mut args = std::env::args().skip(1).filter(|arg| arg != "--bench");
    let frames = args
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(48);
    assert!(frames > 0, "frame count must be positive");
    let max_fps = args.next().map(|value| {
        let fps: u32 = value.parse().expect("FPS cap must be a positive integer");
        assert!(fps > 0, "FPS cap must be positive");
        fps
    });
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("GPU benchmark requires an adapter");
    eprintln!("ADAPTER {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_bind_groups: 4,
            max_non_sampler_bindings: 2048,
            ..Default::default()
        },
        ..Default::default()
    }))
    .unwrap();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("curve benchmark"),
        size: wgpu::Extent3d {
            width: 1024,
            height: 768,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    for stroked in [false, true] {
        for count in [128, 512] {
            for repetition in 0..3 {
                let geometry = fixture(count, stroked);
                let mut backend = backend::Backend::new(&device, &queue);
                for i in 0..12 {
                    let start = Instant::now();
                    backend.render(
                        &device,
                        &queue,
                        std::slice::from_ref(&geometry),
                        Affine::translate((-(i as f64) * 0.75, 0.0)),
                        &view,
                        Size::new(1024, 768),
                    );
                    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    pace(start, max_fps);
                }
                let a = cpu_ticks();
                let total = Instant::now();
                let mut durations = Vec::with_capacity(frames);
                for i in 0..frames {
                    let pan =
                        Affine::translate((-((i % 80) as f64) * 0.75, -((i % 9) as f64) * 0.5));
                    let start = Instant::now();
                    backend.render(
                        &device,
                        &queue,
                        std::slice::from_ref(&geometry),
                        pan,
                        &view,
                        Size::new(1024, 768),
                    );
                    durations.push(start.elapsed().as_secs_f64() * 1000.0);
                    // Bound queue depth and exclude the explicit GPU wait from CPU-stage timing.
                    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    pace(start, max_fps);
                }
                let elapsed = total.elapsed().as_secs_f64();
                let ticks = a.zip(cpu_ticks()).map(|(a, b)| b - a);
                durations.sort_by(f64::total_cmp);
                println!(
                    "{{\"kind\":\"{}\",\"paths\":{},\"repetition\":{},\"frames\":{},\"max_fps\":{},\"achieved_fps\":{},\"cpu_stage_median_ms\":{},\"cpu_stage_p95_ms\":{},\"submit_wait_seconds\":{},\"process_cpu_ticks\":{}}}",
                    if stroked { "stroke" } else { "fill" },
                    count,
                    repetition,
                    frames,
                    max_fps.map_or_else(|| "null".into(), |v| v.to_string()),
                    frames as f64 / elapsed,
                    durations[durations.len() / 2],
                    durations[durations.len() * 95 / 100],
                    elapsed,
                    ticks.map_or_else(|| "null".into(), |v| v.to_string())
                );
            }
        }
    }
}
