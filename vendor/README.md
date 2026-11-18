# Renderer patches

The standalone project vendors two upstream packages with their license and
release provenance files. The root manifest patches them into the dependency
graph so both renderers use Iced's existing WGPU device.

- [iced_wgpu 0.14.0](iced_wgpu) starts at the crates.io release. Its native
  compositor requests four bind groups for Vello GPU. The default-compatible
  `Primitive::prepare_with_encoder` and `Pipeline::after_submit` hooks let
  primitives record into the frame encoder and reclaim staging after window
  and screenshot submissions.
- [vello_gpu 0.4.0](vello_gpu) uses the PoC's [wgpu 27 port](vello_gpu/PATCHES.md).
  It records strip and glyph-atlas passes in the host encoder and recalls
  staging after submission. Ordered atlas uploads preserve cold glyphs when
  multiple canvas layers are prepared in the same frame.

Remove these overrides once the supported Iced release and Vello GPU share
the required wgpu version, device limits, and upload lifecycle. Cargo consumers
must declare the patches at their workspace root until then.
