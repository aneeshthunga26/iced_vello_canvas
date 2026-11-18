//! Translate Iced canvas geometry to a retained Vello scene.
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use iced::{Color, Point, Radians, Rectangle, Size, Vector};
use vello::{
    kurbo::{self, Affine, BezPath, Rect},
    peniko,
};

use crate::scene::Scene;
use crate::{Fill, Gradient, LineCap, LineJoin, Path, Stroke, Style, Text, fill};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// An immutable, cheaply cloned Vello scene in logical canvas coordinates.
#[derive(Clone)]
pub struct Geometry {
    pub(crate) scene: Arc<Scene>,
    pub(crate) id: u64,
    pub(crate) size: Size,
    pub(crate) empty: bool,
    pub(crate) retained: Option<(Rectangle, Rectangle)>,
}

impl std::fmt::Debug for Geometry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Geometry")
            .field("id", &self.id)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Geometry {
    /// Retains this scene in fixed canvas-space raster bounds, displaying only `content`.
    ///
    /// `raster` must enclose `content` and all content must already be encoded. Include
    /// a small gutter for filtered fractional scrolling. Size/scale/content changes
    /// require a new geometry identity. Ordinary canvas geometry remains viewport-cropped.
    ///
    /// # Panics
    /// Panics when raster bounds are empty or do not enclose the content bounds.
    pub fn retained_region(mut self, raster: Rectangle, content: Rectangle) -> Self {
        assert!(raster.width > 0.0 && raster.height > 0.0);
        assert!(raster.intersection(&content) == Some(content));
        self.retained = Some((raster, content));
        self
    }
}

/// A scene builder whose methods mirror the supported Iced canvas frame methods.
///
/// Coordinates and stroke widths are logical pixels; rotations are radians.
pub struct Frame {
    scene: Scene,
    bounds: Rectangle,
    transform: Affine,
    stack: Vec<Affine>,
    drawn: bool,
}

impl Frame {
    /// Creates a frame clipped to the given size, with origin at `(0, 0)`.
    pub fn new<R>(_renderer: &R, size: Size) -> Self {
        Self::with_bounds(_renderer, Rectangle::with_size(size))
    }

    /// Creates a frame clipped to logical bounds without translating its coordinate system.
    pub fn with_bounds<R>(_renderer: &R, bounds: Rectangle) -> Self {
        let mut scene = Scene::new();
        crate::scene::clip_rect(&mut scene, Affine::IDENTITY, rect(bounds));
        Self {
            scene,
            bounds,
            transform: Affine::IDENTITY,
            stack: Vec::new(),
            drawn: false,
        }
    }

    /// Returns the frame width in logical pixels.
    pub fn width(&self) -> f32 {
        self.bounds.width
    }

    /// Returns the frame height in logical pixels.
    pub fn height(&self) -> f32 {
        self.bounds.height
    }

    /// Returns the logical size of the frame's clip bounds.
    pub fn size(&self) -> Size {
        self.bounds.size()
    }

    /// Returns the center of the frame size, matching Iced's frame API.
    pub fn center(&self) -> Point {
        Point::new(self.width() / 2.0, self.height() / 2.0)
    }

    /// Fills a path using its winding rule and solid color or linear gradient.
    pub fn fill(&mut self, path: &Path, fill: impl Into<Fill>) {
        self.drawn = true;
        let fill = fill.into();
        let rule = match fill.rule {
            fill::Rule::NonZero => peniko::Fill::NonZero,
            fill::Rule::EvenOdd => peniko::Fill::EvenOdd,
        };
        self.scene.fill(
            rule,
            self.transform,
            &brush(fill.style),
            None,
            &convert_path(path),
        );
    }

    /// Fills a rectangle in the current coordinate system.
    pub fn fill_rectangle(&mut self, top_left: Point, size: Size, fill: impl Into<Fill>) {
        self.drawn = true;
        let fill = fill.into();
        let rule = match fill.rule {
            fill::Rule::NonZero => peniko::Fill::NonZero,
            fill::Rule::EvenOdd => peniko::Fill::EvenOdd,
        };
        crate::scene::fill_rect(
            &mut self.scene,
            rule,
            self.transform,
            &brush(fill.style),
            rect(Rectangle::new(top_left, size)),
        );
    }

    /// Strokes a path, including caps, joins and dash patterns.
    pub fn stroke<'a>(&mut self, path: &Path, stroke: impl Into<Stroke<'a>>) {
        self.drawn = true;
        let stroke = stroke.into();
        let cap = match stroke.line_cap {
            LineCap::Butt => kurbo::Cap::Butt,
            LineCap::Square => kurbo::Cap::Square,
            LineCap::Round => kurbo::Cap::Round,
        };
        let join = match stroke.line_join {
            LineJoin::Miter => kurbo::Join::Miter,
            LineJoin::Round => kurbo::Join::Round,
            LineJoin::Bevel => kurbo::Join::Bevel,
        };
        let style = kurbo::Stroke::new(f64::from(stroke.width))
            .with_caps(cap)
            .with_join(join);
        let mut path = convert_path(path);
        if !stroke.line_dash.segments.is_empty() {
            // Iced selects the initial alternating interval by index. Odd-length
            // patterns repeat twice before cycling back to the same dash/gap parity.
            let mut segments: Vec<f64> = stroke
                .line_dash
                .segments
                .iter()
                .map(|v| f64::from(*v))
                .collect();
            if segments.len() % 2 == 1 {
                segments.extend_from_within(..);
            }
            let offset = stroke.line_dash.offset % segments.len();
            segments.rotate_left(offset);
            path = kurbo::dash(path.iter(), 0.0, &segments).collect();
        }
        // Iced transforms the centerline before stroking, preserving logical stroke
        // width under Frame::scale. DPI scaling is applied later to the whole scene.
        path.apply_affine(self.transform);
        self.scene.stroke(
            &style,
            Affine::IDENTITY,
            &brush(stroke.style),
            Some(self.transform),
            &path,
        );
    }

    /// Strokes a rectangle in the current coordinate system.
    pub fn stroke_rectangle<'a>(
        &mut self,
        top_left: Point,
        size: Size,
        stroke: impl Into<Stroke<'a>>,
    ) {
        self.stroke(&Path::rectangle(top_left, size), stroke);
    }

    /// Shapes with Iced's loaded fonts, then submits glyph runs to Vello.
    ///
    /// Text follows the current transform and clip. It paints in call order, unlike Iced's
    /// canvas text overlay. Variable font axes and synthetic italic/weight are not adapted.
    pub fn fill_text(&mut self, text: impl Into<Text>) {
        let text = text.into();
        if !text.content.is_empty() {
            self.drawn = true;
            crate::text::draw(&mut self.scene, self.transform, text);
        }
    }

    /// Saves the current transform until the matching `pop_transform`.
    pub fn push_transform(&mut self) {
        self.stack.push(self.transform);
    }

    /// Restores the most recently saved transform.
    ///
    /// # Panics
    /// Panics if there is no matching `push_transform`.
    pub fn pop_transform(&mut self) {
        self.transform = self.stack.pop().expect("Canvas transform stack is empty");
    }

    /// Draws with a saved transform and restores it after the closure returns normally.
    pub fn with_save<T>(&mut self, draw: impl FnOnce(&mut Self) -> T) -> T {
        self.push_transform();
        let result = draw(self);
        self.pop_transform();
        result
    }

    /// Draws in an independent frame clipped to `region`, matching Iced 0.14.
    ///
    /// The child uses canvas coordinates and starts with an identity transform.
    /// It does not inherit the parent transform or shift the origin to the region.
    pub fn with_clip<T>(&mut self, region: Rectangle, draw: impl FnOnce(&mut Self) -> T) -> T {
        let mut child = Frame::with_bounds(&(), region);
        let result = draw(&mut child);
        let geometry = child.into_geometry();
        self.drawn |= !geometry.empty;
        self.scene.append(&geometry.scene, None);
        result
    }

    /// Appends a translation in logical pixels to the current transform.
    pub fn translate(&mut self, translation: Vector) {
        self.transform *= Affine::translate((f64::from(translation.x), f64::from(translation.y)));
    }

    /// Appends a rotation in radians to the current transform.
    pub fn rotate(&mut self, angle: impl Into<Radians>) {
        self.transform *= Affine::rotate(f64::from(angle.into().0));
    }

    /// Appends a uniform scale to the current transform.
    pub fn scale(&mut self, scale: impl Into<f32>) {
        self.transform *= Affine::scale(f64::from(scale.into()));
    }

    /// Appends independent horizontal and vertical scales to the current transform.
    pub fn scale_nonuniform(&mut self, scale: impl Into<Vector>) {
        let scale = scale.into();
        self.transform *= Affine::scale_non_uniform(f64::from(scale.x), f64::from(scale.y));
    }

    /// Finishes the scene and assigns a cache identity shared by its clones.
    pub fn into_geometry(mut self) -> Geometry {
        self.scene.pop_layer();
        let size = self.size();
        Geometry {
            scene: Arc::new(self.scene),
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            size,
            empty: !self.drawn,
            retained: None,
        }
    }
}

fn rect(bounds: Rectangle) -> Rect {
    Rect::new(
        f64::from(bounds.x),
        f64::from(bounds.y),
        f64::from(bounds.x + bounds.width),
        f64::from(bounds.y + bounds.height),
    )
}

fn convert_path(path: &Path) -> BezPath {
    use crate::path::lyon_path::Event;
    let mut result = BezPath::new();
    for event in path.raw().iter() {
        match event {
            Event::Begin { at } => result.move_to((f64::from(at.x), f64::from(at.y))),
            Event::Line { to, .. } => result.line_to((f64::from(to.x), f64::from(to.y))),
            Event::Quadratic { ctrl, to, .. } => result.quad_to(
                (f64::from(ctrl.x), f64::from(ctrl.y)),
                (f64::from(to.x), f64::from(to.y)),
            ),
            Event::Cubic {
                ctrl1, ctrl2, to, ..
            } => result.curve_to(
                (f64::from(ctrl1.x), f64::from(ctrl1.y)),
                (f64::from(ctrl2.x), f64::from(ctrl2.y)),
                (f64::from(to.x), f64::from(to.y)),
            ),
            Event::End { close: true, .. } => result.close_path(),
            Event::End { close: false, .. } => {}
        }
    }
    result
}

/// Converts unpremultiplied Iced sRGB channels to Vello’s sRGB brush color.
pub(crate) fn color(color: Color) -> peniko::Color {
    peniko::Color::new([color.r, color.g, color.b, color.a])
}

fn brush(style: Style) -> peniko::Brush {
    match style {
        Style::Solid(value) => color(value).into(),
        Style::Gradient(Gradient::Linear(value)) => peniko::Gradient::new_linear(
            (f64::from(value.start.x), f64::from(value.start.y)),
            (f64::from(value.end.x), f64::from(value.end.y)),
        )
        .with_stops(
            value
                .stops
                .iter()
                .flatten()
                .map(|s| (s.offset, color(s.color)))
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .into(),
    }
}
