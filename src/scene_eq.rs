//! Exact content comparison for small Vello scenes rebuilt between redraws.

use std::mem::size_of_val;

use vello::{Scene, peniko};
use vello_encoding::{Encoding, GlyphRun, Patch, Resources, StreamOffsets, Transform};

/// Caps the data retained for each layer and the work of comparing two scenes.
pub(crate) fn is_small(scene: &Scene) -> bool {
    const MAX_ENCODING_BYTES: usize = 16 * 1024;
    let encoding = scene.encoding();
    let resources = &encoding.resources;
    size_of_val(encoding.path_data.as_slice())
        + size_of_val(encoding.path_tags.as_slice())
        + size_of_val(encoding.draw_data.as_slice())
        + size_of_val(encoding.draw_tags.as_slice())
        + size_of_val(encoding.transforms.as_slice())
        + size_of_val(encoding.styles.as_slice())
        + size_of_val(resources.glyphs.as_slice())
        + size_of_val(resources.glyph_runs.as_slice())
        + size_of_val(resources.patches.as_slice())
        + size_of_val(resources.color_stops.as_slice())
        + size_of_val(resources.normalized_coords.as_slice())
        <= MAX_ENCODING_BYTES
        && !resources
            .patches
            .iter()
            .any(|patch| matches!(patch, Patch::Image { .. }))
}

/// Checks the commands and their resources before reusing a raster for a new scene.
pub(crate) fn same(previous: &Scene, current: &Scene) -> bool {
    let Encoding {
        path_tags,
        path_data,
        draw_tags,
        draw_data,
        transforms,
        styles,
        resources,
        n_paths,
        n_path_segments,
        n_clips,
        n_open_clips,
        flags,
    } = previous.encoding();
    let current = current.encoding();
    // Check draw data and positions first so animation normally diverges early.
    *draw_data == current.draw_data
        && *path_data == current.path_data
        && *path_tags == current.path_tags
        && *draw_tags == current.draw_tags
        && (*n_paths, *n_path_segments, *n_clips, *n_open_clips, *flags)
            == (
                current.n_paths,
                current.n_path_segments,
                current.n_clips,
                current.n_open_clips,
                current.flags,
            )
        && same_items(transforms, &current.transforms, |a, b| {
            transform(a) == transform(b)
        })
        && same_items(styles, &current.styles, |a, b| {
            a.flags_and_miter_limit == b.flags_and_miter_limit
                && a.line_width.to_bits() == b.line_width.to_bits()
        })
        && same_resources(resources, &current.resources)
}

fn same_resources(previous: &Resources, current: &Resources) -> bool {
    use peniko::color::cache_key::BitEq as _;

    let Resources {
        patches,
        color_stops,
        glyphs,
        glyph_runs,
        normalized_coords,
    } = previous;
    *normalized_coords == current.normalized_coords
        && color_stops
            .as_slice()
            .bit_eq(current.color_stops.as_slice())
        && same_items(glyphs, &current.glyphs, |a, b| {
            a.id == b.id && a.x.to_bits() == b.x.to_bits() && a.y.to_bits() == b.y.to_bits()
        })
        && same_items(glyph_runs, &current.glyph_runs, same_run)
        && same_items(patches, &current.patches, |a, b| match (a, b) {
            (
                Patch::Ramp {
                    draw_data_offset,
                    stops,
                    extend,
                },
                Patch::Ramp {
                    draw_data_offset: other_offset,
                    stops: other_stops,
                    extend: other_extend,
                },
            ) => (draw_data_offset, stops, extend) == (other_offset, other_stops, other_extend),
            (Patch::GlyphRun { index }, Patch::GlyphRun { index: other }) => index == other,
            _ => false,
        })
}

fn same_run(a: &GlyphRun, b: &GlyphRun) -> bool {
    let GlyphRun {
        font,
        transform: run_transform,
        glyph_transform,
        font_size,
        hint,
        normalized_coords,
        style,
        glyphs,
        stream_offsets,
    } = a;
    *font == b.font
        && transform(run_transform) == transform(&b.transform)
        && glyph_transform.as_ref().map(transform) == b.glyph_transform.as_ref().map(transform)
        && font_size.to_bits() == b.font_size.to_bits()
        && *hint == b.hint
        && *normalized_coords == b.normalized_coords
        && matches!((style, &b.style), (peniko::Style::Fill(a), peniko::Style::Fill(b)) if a == b)
        && *glyphs == b.glyphs
        && offsets(stream_offsets) == offsets(&b.stream_offsets)
}

fn transform(transform: &Transform) -> ([u32; 4], [u32; 2]) {
    let Transform {
        matrix,
        translation,
    } = transform;
    (matrix.map(f32::to_bits), translation.map(f32::to_bits))
}

fn offsets(offsets: &StreamOffsets) -> [usize; 6] {
    let StreamOffsets {
        path_tags,
        path_data,
        draw_tags,
        draw_data,
        transforms,
        styles,
    } = *offsets;
    [
        path_tags, path_data, draw_tags, draw_data, transforms, styles,
    ]
}

fn same_items<T>(a: &[T], b: &[T], same: impl Fn(&T, &T) -> bool) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vello::kurbo::{Affine, Rect};

    #[test]
    fn compares_gradient_stops_even_when_draw_commands_match() {
        let scene = |color: peniko::Color| {
            let mut scene = Scene::new();
            let gradient = peniko::Gradient::new_linear((0.0, 0.0), (24.0, 0.0))
                .with_stops([(0.0, peniko::Color::TRANSPARENT), (1.0, color)].as_slice());
            scene.fill(
                peniko::Fill::NonZero,
                Affine::IDENTITY,
                &gradient,
                None,
                &Rect::new(0.0, 0.0, 24.0, 24.0),
            );
            scene
        };

        let red = peniko::Color::new([1.0, 0.0, 0.0, 1.0]);
        let blue = peniko::Color::new([0.0, 0.0, 1.0, 1.0]);
        let first = scene(red);
        let changed = scene(blue);
        assert_eq!(first.encoding().draw_data, changed.encoding().draw_data);
        assert!(is_small(&first));
        assert!(same(&first, &scene(red)));
        assert!(!same(&first, &changed));
    }
}
