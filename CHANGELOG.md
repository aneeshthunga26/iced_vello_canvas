# Changelog

## Unreleased

- Added a live Bézier curve FPS comparison with side-by-side and solo modes.
- Default to the proof of concept's Vello GPU strip renderer. The compute
  renderer remains available when the `gpu` feature is disabled.
- Carry the wgpu 27 compatibility port and Iced submission hooks needed by the
  strip renderer. Disable crates.io publication while those patches are required.
- Extracted the native Vello canvas widget for Iced 0.14 with paths, text,
  clipping, transforms and retained raster regions.
- Added a minimal usage example and retained the side-by-side Iced comparison.
