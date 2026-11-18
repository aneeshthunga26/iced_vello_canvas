# Local wgpu 27 compatibility port

Source: the crates.io `vello_gpu` 0.4.0 release, upstream revision
`b7e8f523b45c07a5edb806bb7241ef5afe34a882` (`vello_gpu` package).
The upstream MIT and Apache-2.0 licenses are retained. `Cargo.toml.orig` records
the released workspace manifest. Only the library sources are included.

Upstream uses wgpu 30; Iced 0.14 uses wgpu 27. This experiment changes the
wgpu dependency to 27.0.1 and adapts pipeline/pass descriptors, buffer mapping
views and staging-belt signatures. No rasterizer, scheduler, shader or text
algorithm is replaced. The staging belt retains a device handle and exposes
`Renderer::after_submit()` for the wgpu 27 finish/submit/recall lifecycle;
call it after submitting the host encoder, which can contain multiple renders.
Atlas rendering now records into that same encoder. Bitmap glyph uploads and
atlas clears use padded, encoder-ordered buffer copies instead of queue writes,
so reuse cannot overwrite atlas data referenced by an earlier pending scene.
The temporary upload buffers remain alive through command submission. The
renderer performs no internal submission or recall. The absent packaged example declaration is removed.

The native Iced device request is raised from two to four bind groups for
Vello GPU's four-group strip pipeline; browser limits stay at two. Headless
native devices use four too. The compute backend needs no such limit change.
All fresh benchmark controls use the same native limits. This dependency
port and host limit requirement must be resolved before publishing a
standalone drop-in crate. Remove the override when Iced and a supported
Vello GPU release share a wgpu version and compatible device limits.

Validation: shared-device application compilation and actual GPU pixel tests
cover analytic edge coverage, text ink, clipping, alpha/color, logical stroke
width, DPI changes, huge canvas crops, independent canvases, layer batching,
unchanged raster reuse and changed-content output allocation reuse.

Retained rendering validation also covers adjacent padded regions with cold
glyphs recorded before a single host submission, independent overlay updates,
fractional pan reuse, and DPI/expiry reconstruction. The host must call
`after_submit` after submission, not between scenes recorded into that encoder.
