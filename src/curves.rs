//! Bounded translation-independent reuse of Vello's native stroke preparation.
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};
use vello::kurbo::{Affine, BezPath, PathEl, Point, Rect, Stroke, StrokeCtx, Vec2};
use vello_common::{
    fearless_simd::Level,
    flatten::{self, FlattenCtx, Line},
    geometry::RectU16,
};

const MAX_ENTRIES: usize = 256;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_SOURCE_ELEMENTS: usize = 1024;
const MAX_PREPARED_POINTS: usize = 32_768;
const MAX_SPAN: f64 = 4096.0;
// Matches the stroke-expansion tolerance in the pinned vello_common 0.4 flatten::stroke.
const STROKE_TOLERANCE: f64 = 0.25;

struct Prepared {
    path: BezPath,
    shift: Vec2,
}
struct Entry {
    source: BezPath,
    linear: [u64; 4],
    stroke: Stroke,
    prepared: Arc<Prepared>,
    used: u64,
    bytes: usize,
}

/// Prepared polygon edges with a fresh device-space placement and independent paint mapping.
pub(crate) struct Draw {
    prepared: Arc<Prepared>,
    placement: Vec2,
}
impl Draw {
    /// Returns closed, already flattened edges; native clipping/tiling/AA still runs per view.
    pub(crate) fn path(&self) -> &BezPath {
        &self.prepared.path
    }
    /// Places the canonical edges without applying their linear transform twice.
    pub(crate) fn transform(&self) -> Affine {
        Affine::translate(self.placement)
    }
    /// Keeps paint in its original coordinate system after moving linear work into geometry.
    pub(crate) fn paint_transform(&self, original: Affine) -> Affine {
        Affine::translate(-self.placement) * original
    }
}

/// A backend-owned cache keyed by exact normalized geometry, linear transform and stroke style.
pub(crate) struct CurveCache {
    entries: HashMap<u64, Vec<Entry>>,
    count: usize,
    bytes: usize,
    clock: u64,
    hits: u64,
    preparations: u64,
    level: Level,
    flatten: FlattenCtx,
    stroke: StrokeCtx,
    lines: Vec<Line>,
    #[cfg(test)]
    disabled: bool,
}
impl Default for CurveCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            count: 0,
            bytes: 0,
            clock: 0,
            hits: 0,
            preparations: 0,
            level: Level::try_detect().unwrap_or(Level::baseline()),
            flatten: FlattenCtx::default(),
            stroke: StrokeCtx::default(),
            lines: Vec::new(),
            #[cfg(test)]
            disabled: false,
        }
    }
}
impl CurveCache {
    /// Reuses complete stroked edges across translation, paint and clip changes; unsafe sizes fall back.
    pub(crate) fn resolve(
        &mut self,
        path: &BezPath,
        style: &Stroke,
        transform: Affine,
    ) -> Option<Draw> {
        #[cfg(test)]
        if self.disabled {
            return None;
        }
        if path.elements().len() > MAX_SOURCE_ELEMENTS || !path.is_finite() {
            return None;
        }
        if !style.width.is_finite()
            || style.width <= 0.0
            || !style.miter_limit.is_finite()
            || !style.dash_pattern.is_empty()
        {
            return None;
        }
        let [a, b, c, d, e, f] = transform.as_coeffs();
        if ![a, b, c, d, e, f].iter().all(|x| x.is_finite()) || a * d - b * c == 0.0 {
            return None;
        }
        let linear = Affine::new([a, b, c, d, 0.0, 0.0]);
        let bits = [a.to_bits(), b.to_bits(), c.to_bits(), d.to_bits()];
        let bbox = path.control_box();
        let origin = bbox.origin();
        let bounds = linear.transform_rect_bbox(Rect::new(0.0, 0.0, bbox.width(), bbox.height()));
        if !cacheable(bounds) {
            return None;
        }
        let hash = fingerprint(path, origin, bits, style);
        self.clock += 1;
        if let Some(bucket) = self.entries.get_mut(&hash) {
            for entry in bucket {
                if entry.linear == bits
                    && entry.stroke == *style
                    && entry
                        .source
                        .iter()
                        .eq(path.iter().map(|el| normalized(el, origin)))
                {
                    entry.used = self.clock;
                    self.hits += 1;
                    return Some(Draw {
                        prepared: Arc::clone(&entry.prepared),
                        placement: (transform * origin).to_vec2() - entry.prepared.shift,
                    });
                }
            }
        }
        let source: BezPath = path.iter().map(|el| normalized(el, origin)).collect();
        flatten::expand_stroke(
            source.iter(),
            style,
            STROKE_TOLERANCE / a.abs().max(d.abs()).max(1.0),
            &mut self.stroke,
        );
        let geometry = self.stroke.output().clone();
        if geometry.elements().len() > MAX_PREPARED_POINTS {
            self.stroke = StrokeCtx::default();
            return None;
        }
        let bounds = linear.transform_rect_bbox(geometry.control_box());
        if !cacheable(bounds) {
            return None;
        }
        let shift = Vec2::new(2.0 - bounds.x0, 2.0 - bounds.y0);
        // The entire control box is inside this canonical viewport. No edges may be
        // culled here: later views need complete winding, including offscreen edges.
        flatten::fill(
            self.level,
            geometry.iter(),
            Affine::translate(shift) * linear,
            &mut self.lines,
            &mut self.flatten,
            RectU16::new(0, 0, u16::MAX, u16::MAX),
        );
        if self.lines.len() > MAX_PREPARED_POINTS {
            self.lines = Vec::new();
            self.flatten = FlattenCtx::default();
            return None;
        }
        let flattened = closed_edges(&self.lines)?;
        let bytes = (source.elements().len() + flattened.elements().len())
            * std::mem::size_of::<PathEl>()
            * 2
            + style.dash_pattern.len() * 8
            + 256;
        if bytes > MAX_BYTES {
            return None;
        }
        while self.count >= MAX_ENTRIES || self.bytes + bytes > MAX_BYTES {
            self.evict();
        }
        self.preparations += 1;
        let prepared = Arc::new(Prepared {
            path: flattened,
            shift,
        });
        self.entries.entry(hash).or_default().push(Entry {
            source,
            linear: bits,
            stroke: style.clone(),
            prepared: Arc::clone(&prepared),
            used: self.clock,
            bytes,
        });
        self.count += 1;
        self.bytes += bytes;
        Some(Draw {
            placement: (transform * origin).to_vec2() - prepared.shift,
            prepared,
        })
    }
    fn evict(&mut self) {
        let (hash, index) = self
            .entries
            .iter()
            .flat_map(|(h, b)| b.iter().enumerate().map(move |(i, e)| (*h, i, e.used)))
            .min_by_key(|(_, _, used)| *used)
            .map(|(h, i, _)| (h, i))
            .expect("nonempty curve cache");
        let bucket = self.entries.get_mut(&hash).expect("curve bucket exists");
        let entry = bucket.swap_remove(index);
        if bucket.is_empty() {
            self.entries.remove(&hash);
        }
        self.count -= 1;
        self.bytes -= entry.bytes;
    }
    /// Enables native-path comparison in GPU tests without changing consumer APIs.
    #[cfg(test)]
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.disabled = !enabled;
    }
    /// Reports deterministic reuse and retained budgets for cache regression tests.
    #[cfg(test)]
    pub(crate) fn stats(&self) -> (u64, u64, usize, usize) {
        (self.hits, self.preparations, self.count, self.bytes)
    }
}
fn cacheable(bounds: Rect) -> bool {
    bounds.is_finite() && bounds.width() <= MAX_SPAN && bounds.height() <= MAX_SPAN
}
fn normalized(el: PathEl, origin: Point) -> PathEl {
    let offset = -origin.to_vec2();
    match el {
        PathEl::MoveTo(p) => PathEl::MoveTo(p + offset),
        PathEl::LineTo(p) => PathEl::LineTo(p + offset),
        PathEl::QuadTo(p, q) => PathEl::QuadTo(p + offset, q + offset),
        PathEl::CurveTo(p, q, r) => PathEl::CurveTo(p + offset, q + offset, r + offset),
        PathEl::ClosePath => PathEl::ClosePath,
    }
}
fn fingerprint(path: &BezPath, origin: Point, linear: [u64; 4], stroke: &Stroke) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    linear.hash(&mut h);
    stroke.width.to_bits().hash(&mut h);
    stroke.miter_limit.to_bits().hash(&mut h);
    (stroke.join as u8).hash(&mut h);
    (stroke.start_cap as u8).hash(&mut h);
    (stroke.end_cap as u8).hash(&mut h);
    stroke.dash_offset.to_bits().hash(&mut h);
    stroke.dash_pattern.len().hash(&mut h);
    for v in &stroke.dash_pattern {
        v.to_bits().hash(&mut h);
    }
    for el in path.iter().map(|el| normalized(el, origin)) {
        let mut point = |p: Point| {
            p.x.to_bits().hash(&mut h);
            p.y.to_bits().hash(&mut h);
        };
        match el {
            PathEl::MoveTo(p) => {
                point(p);
                0_u8.hash(&mut h);
            }
            PathEl::LineTo(p) => {
                point(p);
                1_u8.hash(&mut h);
            }
            PathEl::QuadTo(p, q) => {
                point(p);
                point(q);
                2_u8.hash(&mut h);
            }
            PathEl::CurveTo(p, q, r) => {
                point(p);
                point(q);
                point(r);
                3_u8.hash(&mut h);
            }
            PathEl::ClosePath => 4_u8.hash(&mut h),
        }
    }
    h.finish()
}
fn closed_edges(lines: &[Line]) -> Option<BezPath> {
    let mut result = BezPath::new();
    let mut start = None;
    let mut last = None;
    for line in lines {
        let a = Point::new(f64::from(line.p0.x), f64::from(line.p0.y));
        let b = Point::new(f64::from(line.p1.x), f64::from(line.p1.y));
        if last != Some(a) {
            if start.is_some() {
                return None;
            }
            result.move_to(a);
            start = Some(a);
        }
        result.line_to(b);
        last = Some(b);
        if start == Some(b) {
            result.close_path();
            start = None;
            last = None;
        }
    }
    if start.is_some() { None } else { Some(result) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn curve() -> BezPath {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.curve_to((10.0, -20.0), (40.0, 70.0), (50.0, 30.0));
        p.close_path();
        p
    }
    #[test]
    fn reuses_translation_and_invalidates_linear_transform_and_stroke_style() {
        let mut cache = CurveCache::default();
        let path = curve();
        let stroke = Stroke::new(2.0);
        let a = cache.resolve(&path, &stroke, Affine::IDENTITY).unwrap();
        let mut shifted = path.clone();
        shifted.apply_affine(Affine::translate((50_000.0, -80.0)));
        let b = cache
            .resolve(&shifted, &stroke, Affine::translate((-50_000.25, 80.5)))
            .unwrap();
        assert!(Arc::ptr_eq(&a.prepared, &b.prepared));
        assert_eq!(cache.stats().0, 1);
        cache.resolve(&path, &stroke, Affine::scale(2.0)).unwrap();
        cache
            .resolve(&path, &Stroke::new(3.0), Affine::IDENTITY)
            .unwrap();
        assert_eq!(cache.stats().1, 3);
    }
    #[test]
    fn bounds_retention_and_falls_back_for_large_or_incomplete_shapes() {
        let mut cache = CurveCache::default();
        for i in 0..MAX_ENTRIES + 10 {
            let mut path = BezPath::new();
            path.move_to((0.0, 0.0));
            path.curve_to((10.0, -20.0), (40.0 + i as f64 * 0.125, 70.0), (50.0, 30.0));
            path.close_path();
            cache.resolve(&path, &Stroke::new(2.0), Affine::IDENTITY);
        }
        assert_eq!(cache.count, MAX_ENTRIES);
        assert!(cache.bytes <= MAX_BYTES);
        let mut huge = curve();
        huge.apply_affine(Affine::scale(10_000.0));
        assert!(
            cache
                .resolve(&huge, &Stroke::new(2.0), Affine::IDENTITY)
                .is_none()
        );
        let incomplete = [Line::new(
            flatten::Point::ZERO,
            flatten::Point::new(1.0, 1.0),
        )];
        assert!(closed_edges(&incomplete).is_none());
    }
    #[test]
    fn byte_budget_evicts_large_preparations_before_the_entry_limit() {
        let mut cache = CurveCache::default();
        for i in 0..50 {
            let mut path = BezPath::new();
            for j in 0..12 {
                let y = j as f64 * 0.25;
                path.move_to((0.0, y));
                path.curve_to(
                    (0.0, 1000.0),
                    (1000.0 + i as f64 * 0.125, 1000.0),
                    (1000.0, y),
                );
                path.curve_to((1000.0, -1000.0), (0.0, -1000.0), (0.0, y));
                path.close_path();
            }
            cache
                .resolve(&path, &Stroke::new(2.0), Affine::IDENTITY)
                .expect("bounded complex stroke can be prepared");
        }
        assert_eq!(cache.preparations, 50);
        assert!(
            cache.count < 50 && cache.bytes <= MAX_BYTES,
            "byte pressure must evict entries: {} / {}",
            cache.count,
            cache.bytes
        );
    }
}
