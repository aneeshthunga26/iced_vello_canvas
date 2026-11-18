//! Bounded shaped-text reuse over Iced's font database, with native Vello glyph rendering.
use std::{cell::RefCell, collections::HashMap, sync::Arc};

use iced::{
    Font, Size,
    advanced::{
        graphics::text,
        text::{Alignment, Paragraph as _, Shaping, Wrapping},
    },
    alignment,
};
use text::cosmic_text::fontdb;
use vello::{
    Glyph,
    kurbo::Affine,
    peniko::{Blob, FontData},
};

use crate::{Text, scene::Scene};

thread_local! {
    // Keep font blob identity stable across geometry and shaped-run cache eviction.
    static FONTS: RefCell<HashMap<fontdb::ID, FontData>> = RefCell::default();
    static SHAPES: RefCell<ShapeCache> = RefCell::default();
}

const MAX_ENTRIES: usize = 256;
const MAX_GLYPHS: usize = 16_384;
const MAX_CONTENT_BYTES: usize = 65_536;
const MAX_CACHEABLE_TEXT: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Attributes {
    font: Font,
    size: u32,
    line_height: u32,
    width: u32,
    align_x: Alignment,
    align_y: alignment::Vertical,
    shaping: Shaping,
}
impl Attributes {
    fn new(value: &Text) -> Self {
        Self {
            font: value.font,
            size: value.size.0.to_bits(),
            line_height: value.line_height.to_absolute(value.size).0.to_bits(),
            width: value.max_width.to_bits(),
            align_x: value.align_x,
            align_y: value.align_y,
            shaping: value.shaping,
        }
    }
}

struct Run {
    font: FontData,
    size: f32,
    glyphs: Arc<[Glyph]>,
}
struct ShapedText {
    runs: Vec<Run>,
    glyph_count: usize,
}
struct Entry {
    shaped: Arc<ShapedText>,
    used: u64,
}

#[derive(Default)]
struct ShapeCache {
    version: Option<text::Version>,
    entries: HashMap<Attributes, HashMap<String, Entry>>,
    clock: u64,
    count: usize,
    glyphs: usize,
    bytes: usize,
}
impl ShapeCache {
    fn resolve(&mut self, value: &Text, version: text::Version) -> Arc<ShapedText> {
        if self.version != Some(version) {
            *self = Self {
                version: Some(version),
                ..Self::default()
            };
        }
        self.clock += 1;
        let attributes = Attributes::new(value);
        if let Some(entry) = self
            .entries
            .get_mut(&attributes)
            .and_then(|texts| texts.get_mut(value.content.as_str()))
        {
            entry.used = self.clock;
            return Arc::clone(&entry.shaped);
        }
        let shaped = Arc::new(shape(value));
        if value.content.len() > MAX_CACHEABLE_TEXT || shaped.glyph_count > MAX_GLYPHS {
            return shaped;
        }
        while self.count >= MAX_ENTRIES
            || self.glyphs + shaped.glyph_count > MAX_GLYPHS
            || self.bytes + value.content.len() > MAX_CONTENT_BYTES
        {
            self.evict_oldest();
        }
        self.count += 1;
        self.glyphs += shaped.glyph_count;
        self.bytes += value.content.len();
        self.entries.entry(attributes).or_default().insert(
            value.content.clone(),
            Entry {
                shaped: Arc::clone(&shaped),
                used: self.clock,
            },
        );
        shaped
    }
    fn evict_oldest(&mut self) {
        let (attributes, content) = self
            .entries
            .iter()
            .flat_map(|(attributes, texts)| {
                texts
                    .iter()
                    .map(move |(content, entry)| (*attributes, content, entry.used))
            })
            .min_by_key(|(_, _, used)| *used)
            .map(|(a, c, _)| (a, c.clone()))
            .expect("nonempty bounded text cache");
        let texts = self
            .entries
            .get_mut(&attributes)
            .expect("text attribute group exists");
        let entry = texts.remove(&content).expect("oldest text exists");
        if texts.is_empty() {
            self.entries.remove(&attributes);
        }
        self.count -= 1;
        self.glyphs -= entry.shaped.glyph_count;
        self.bytes -= content.len();
    }
}

/// Reuses shape/alignment results independently of position, paint, canvas crop and DPI.
pub(crate) fn draw(scene: &mut Scene, transform: Affine, value: Text) {
    let version = text::font_system()
        .read()
        .expect("Iced font database poisoned")
        .version();
    let shaped = SHAPES.with_borrow_mut(|cache| cache.resolve(&value, version));
    let transform =
        transform * Affine::translate((f64::from(value.position.x), f64::from(value.position.y)));
    for run in &shaped.runs {
        scene.glyphs(
            &run.font,
            run.size,
            transform,
            crate::frame::color(value.color),
            Arc::clone(&run.glyphs),
        );
    }
}

fn shape(value: &Text) -> ShapedText {
    let paragraph = text::Paragraph::with_text(iced::advanced::text::Text {
        content: &value.content,
        bounds: Size::new(value.max_width, f32::INFINITY),
        size: value.size,
        line_height: value.line_height,
        font: value.font,
        align_x: value.align_x,
        align_y: value.align_y,
        shaping: value.shaping,
        wrapping: Wrapping::default(),
    });
    let x = -match value.align_x {
        Alignment::Center => paragraph.min_width() / 2.0,
        Alignment::Right => paragraph.min_width(),
        _ => 0.0,
    };
    let y = -match value.align_y {
        alignment::Vertical::Top => 0.0,
        alignment::Vertical::Center => paragraph.min_height() / 2.0,
        alignment::Vertical::Bottom => paragraph.min_height(),
    };
    let mut system = text::font_system()
        .write()
        .expect("Iced font database poisoned");
    FONTS.with_borrow_mut(|fonts| {
        let mut runs = Vec::new();
        let mut glyph_count = 0;
        for run in paragraph.buffer().layout_runs() {
            let mut start = 0;
            while start < run.glyphs.len() {
                let first = &run.glyphs[start];
                let end = start
                    + run.glyphs[start..]
                        .iter()
                        .take_while(|g| {
                            g.font_id == first.font_id && g.font_size == first.font_size
                        })
                        .count();
                let font = fonts.entry(first.font_id).or_insert_with(|| {
                    system
                        .raw()
                        .db()
                        .with_face_data(first.font_id, |bytes, index| {
                            FontData::new(Blob::new(Arc::new(bytes.to_vec())), index)
                        })
                        .expect("Shaped font must remain in Iced's database")
                });
                let glyphs: Arc<[Glyph]> = run.glyphs[start..end]
                    .iter()
                    .map(|g| Glyph {
                        id: u32::from(g.glyph_id),
                        x: x + g.x + g.font_size * g.x_offset,
                        y: y + run.line_y + g.y - g.font_size * g.y_offset,
                    })
                    .collect();
                glyph_count += glyphs.len();
                runs.push(Run {
                    font: font.clone(),
                    size: first.font_size,
                    glyphs,
                });
                start = end;
            }
        }
        ShapedText { runs, glyph_count }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shape_reuse_ignores_position_and_color_but_tracks_layout_and_font_version() {
        let mut cache = ShapeCache::default();
        let version = text::Version::default();
        let mut value = Text {
            content: "ffi café 12".into(),
            ..Text::default()
        };
        let first = cache.resolve(&value, version);
        value.position.x = 120.0;
        value.color = iced::Color::WHITE;
        assert!(Arc::ptr_eq(&first, &cache.resolve(&value, version)));
        value.size = 24.0.into();
        assert!(!Arc::ptr_eq(&first, &cache.resolve(&value, version)));
        let sized = cache.resolve(&value, version);
        value.max_width = 40.0;
        assert!(!Arc::ptr_eq(&sized, &cache.resolve(&value, version)));
        let wrapped = cache.resolve(&value, version);
        value.align_y = alignment::Vertical::Bottom;
        assert!(!Arc::ptr_eq(&wrapped, &cache.resolve(&value, version)));
        let aligned = cache.resolve(&value, version);
        // Simulates an Iced font publication without mutating the global font database.
        cache.version = None;
        let reset = cache.resolve(&value, version);
        assert!(!Arc::ptr_eq(&aligned, &reset));
        assert_eq!(cache.count, 1);
    }
    #[test]
    fn shaped_cache_evicts_unused_entries_and_bypasses_large_paragraphs() {
        let mut cache = ShapeCache::default();
        let version = text::Version::default();
        for i in 0..MAX_ENTRIES + 10 {
            cache.resolve(
                &Text {
                    content: i.to_string(),
                    ..Text::default()
                },
                version,
            );
        }
        assert_eq!(cache.count, MAX_ENTRIES);
        let attributes = Attributes::new(&Text::default());
        assert!(!cache.entries[&attributes].contains_key("0"));
        let large = Text {
            content: "a".repeat(MAX_CACHEABLE_TEXT + 1),
            ..Text::default()
        };
        let first = cache.resolve(&large, version);
        assert!(!Arc::ptr_eq(&first, &cache.resolve(&large, version)));
        assert_eq!(cache.count, MAX_ENTRIES);
        assert!(cache.glyphs <= MAX_GLYPHS && cache.bytes <= MAX_CONTENT_BYTES);
    }
}
