# iced_vello_canvas

A native canvas widget for [Iced 0.14](https://iced.rs/) that rasterizes paths
and text with [Vello GPU](https://github.com/linebender/vello/tree/main/vello_gpu)
on Iced's existing WGPU device. The crate implements a subset of
`iced::widget::canvas` and retains raster output across redraws and panning.

## Getting started

Build the examples from a checkout of this repository. To use the widget in an
app, put the two dependency patches in the app's root `Cargo.toml` (or its
workspace root). Cargo reads patches from the consuming workspace.

```toml
[dependencies]
iced = "0.14"
iced_vello_canvas = { git = "https://github.com/aneeshthunga26/iced_vello_canvas" }

[patch.crates-io]
iced_wgpu = { git = "https://github.com/aneeshthunga26/iced_vello_canvas" }
vello_gpu = { git = "https://github.com/aneeshthunga26/iced_vello_canvas" }
```

For a canvas program using the supported API, import this crate as `canvas`.

```rust
use iced_vello_canvas as canvas;
```

Keep the usual `canvas::Program<Message>` implementation, `Frame::new(renderer,
bounds.size())` and `Vec<canvas::Geometry>` return value. Add the widget to your
view using `canvas::Canvas::new(program)` or `canvas::canvas(program)`.

- [Basic example](examples/basic.rs) draws a circle in an Iced window.
- [Comparison example](examples/comparison.rs) draws the same scene in Iced
  and Vello. Left-click either canvas to zoom and right-click to change path
  density.
- [Bézier FPS example](examples/bezier_fps.rs) animates 1,000, 3,000 or 10,000
  cubic Bézier strokes per canvas.

```sh
cargo run --locked --example basic
cargo run --locked --release --example comparison
cargo run --locked --release --example bezier_fps
```

The Bézier example starts side by side, with one FPS counter for the shared
window. Switch between **Iced only** and **Vello only** to measure each renderer
with the same paths and viewport size. The counter averages redraws over at
least half a second. Iced uses its antialiasing setting and Vello uses its native
antialiasing. The example requests VSync off, though your compositor or GPU
backend may still limit the redraw rate.

## Compatibility

Use Iced 0.14 with its WGPU renderer (wgpu 27). The default `gpu` feature uses
Vello GPU's sparse-strip renderer and requires a native adapter that supports
four bind groups. Disable `gpu` to use Vello 0.7's compute renderer, which
requires compute shaders. On Linux, select `x11` or `wayland`. The default
features enable `gpu` and `x11`.

```sh
cargo run --locked --no-default-features --features gpu,wayland --example basic
cargo run --locked --no-default-features --features x11 --example comparison
```

The [vendored patches](vendor/README.md) let Vello GPU 0.4 share Iced's wgpu 27
device and record work into Iced's frame encoder. The Git checkout is usable
with those patches. Crates.io publication is disabled while the patches
are required.

| API | Supported operations |
| --- | --- |
| `Canvas` and `Program` | Width and height, custom themes, per-widget state, input events, actions and cursor interactions |
| `Frame` | Filled and stroked paths and rectangles, linear gradients, winding rules, dashes, text, transforms and rectangular clips |
| `Cache` and `Geometry` | Size- or bounds-based caching, explicit invalidation with `Cache::clear` and fixed raster regions for panning |

Text uses Iced's loaded fonts and shaping, follows transforms and clipping, and
paints in call order. Variable font axes and synthetic font styles are outside
the supported subset. Frame transforms use logical pixels and radians. Scaling
transforms stroke paths while keeping logical stroke widths. A clipped child
frame starts with an identity transform in canvas coordinates.

`Cache::with_group`, renderer type parameters on geometry/frame/cache, images
and SVG are outside the supported subset. TinySkia, WebGL and browser WebGPU
are unsupported. Vello initialization and rendering errors currently panic.

## Development

```sh
cargo fmt --all --check
cargo test --locked
cargo test --locked --lib -- --include-ignored
cargo test --locked --no-default-features --features x11 --lib -- --include-ignored
cargo check --locked --all-targets
```

The `--include-ignored` commands run offscreen pixel tests on a native WGPU
adapter. They cover clipping, alpha blending, text, large drawing crops and
retained regions during panning and DPI changes. The default suite also checks
that cached stroke outlines preserve the strip renderer's pixels.

Run the GPU curve benchmark with `cargo bench --locked --bench curves -- 48 1000`
for 48 measured frames per repetition and a 1,000 FPS cap. Omit the cap for
uncapped runs.

## License

[MIT](LICENSE), including the notice for the widget lifecycle and program API
adapted from Iced. The vendored dependencies retain their upstream license files.
