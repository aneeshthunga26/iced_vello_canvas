//! Backend-specific scene encoding for canvas geometry.
use std::sync::Arc;

use vello::{
    Glyph,
    kurbo::{Affine, BezPath, Rect, Stroke},
    peniko::{Brush, Color, Fill, FontData},
};

/// The renderer that rasterizes a canvas frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RendererBackend {
    /// Vello's strip renderer, which prepares coverage on the CPU.
    #[cfg(feature = "gpu")]
    #[default]
    Gpu,
    /// Vello's area-antialiasing compute renderer, available with the `compute`
    /// feature or in builds without the `gpu` feature.
    #[cfg(any(feature = "compute", not(feature = "gpu")))]
    #[cfg_attr(not(feature = "gpu"), default)]
    Compute,
}

/// A scene encoded directly for its selected renderer.
pub(crate) enum Scene {
    #[cfg(feature = "gpu")]
    Gpu(gpu::Scene),
    #[cfg(any(feature = "compute", not(feature = "gpu")))]
    Compute(Box<vello::Scene>),
}

impl Scene {
    /// Starts an empty scene for the selected backend.
    pub(crate) fn new(backend: RendererBackend) -> Self {
        match backend {
            #[cfg(feature = "gpu")]
            RendererBackend::Gpu => Self::Gpu(gpu::Scene::new()),
            #[cfg(any(feature = "compute", not(feature = "gpu")))]
            RendererBackend::Compute => Self::Compute(Box::default()),
        }
    }

    /// Identifies the renderer that consumes this scene.
    pub(crate) fn backend(&self) -> RendererBackend {
        match self {
            #[cfg(feature = "gpu")]
            Self::Gpu(_) => RendererBackend::Gpu,
            #[cfg(any(feature = "compute", not(feature = "gpu")))]
            Self::Compute(_) => RendererBackend::Compute,
        }
    }

    /// Encodes a filled path.
    pub(crate) fn fill(&mut self, rule: Fill, transform: Affine, brush: &Brush, path: &BezPath) {
        match self {
            #[cfg(feature = "gpu")]
            Self::Gpu(scene) => scene.fill(rule, transform, brush, path),
            #[cfg(any(feature = "compute", not(feature = "gpu")))]
            Self::Compute(scene) => scene.fill(rule, transform, brush, None, path),
        }
    }

    /// Encodes a filled rectangle without building a generic path.
    pub(crate) fn fill_rect(&mut self, rule: Fill, transform: Affine, brush: &Brush, rect: Rect) {
        match self {
            #[cfg(feature = "gpu")]
            Self::Gpu(scene) => scene.fill_rect(rule, transform, brush, rect),
            #[cfg(any(feature = "compute", not(feature = "gpu")))]
            Self::Compute(scene) => scene.fill(rule, transform, brush, None, &rect),
        }
    }

    /// Encodes a stroke with an independent paint transform.
    pub(crate) fn stroke(
        &mut self,
        stroke: &Stroke,
        transform: Affine,
        brush: &Brush,
        brush_transform: Affine,
        path: BezPath,
    ) {
        match self {
            #[cfg(feature = "gpu")]
            Self::Gpu(scene) => scene.stroke(stroke, transform, brush, brush_transform, path),
            #[cfg(any(feature = "compute", not(feature = "gpu")))]
            Self::Compute(scene) => {
                scene.stroke(stroke, transform, brush, Some(brush_transform), &path)
            }
        }
    }

    /// Encodes a shaped glyph run with its retained font.
    pub(crate) fn glyphs(
        &mut self,
        font: &FontData,
        size: f32,
        transform: Affine,
        color: Color,
        glyphs: Arc<[Glyph]>,
    ) {
        match self {
            #[cfg(feature = "gpu")]
            Self::Gpu(scene) => scene.glyphs(font, size, transform, color, glyphs),
            #[cfg(any(feature = "compute", not(feature = "gpu")))]
            Self::Compute(scene) => scene
                .draw_glyphs(font)
                .font_size(size)
                .transform(transform)
                .brush(color)
                .draw(Fill::NonZero, glyphs.iter().copied()),
        }
    }

    /// Starts a rectangular clip.
    pub(crate) fn clip_rect(&mut self, transform: Affine, rect: Rect) {
        match self {
            #[cfg(feature = "gpu")]
            Self::Gpu(scene) => scene.clip_rect(transform, rect),
            #[cfg(any(feature = "compute", not(feature = "gpu")))]
            Self::Compute(scene) => scene.push_clip_layer(Fill::NonZero, transform, &rect),
        }
    }

    /// Ends the innermost clip.
    pub(crate) fn pop_layer(&mut self) {
        match self {
            #[cfg(feature = "gpu")]
            Self::Gpu(scene) => scene.pop_layer(),
            #[cfg(any(feature = "compute", not(feature = "gpu")))]
            Self::Compute(scene) => scene.pop_layer(),
        }
    }

    /// Appends the contents of a child frame encoded for this backend.
    pub(crate) fn append(&mut self, child: &Self) {
        match (self, child) {
            #[cfg(feature = "gpu")]
            (Self::Gpu(scene), Self::Gpu(child)) => scene.append(child),
            #[cfg(any(feature = "compute", not(feature = "gpu")))]
            (Self::Compute(scene), Self::Compute(child)) => scene.append(child, None),
            #[cfg(all(feature = "compute", feature = "gpu"))]
            _ => unreachable!("clipped child frames inherit their parent's backend"),
        }
    }
}

#[cfg(feature = "gpu")]
mod gpu {
    use std::sync::Arc;
    use vello::{
        Glyph,
        kurbo::{Affine, BezPath, Cap, PathEl, Rect, Stroke},
        peniko::{Brush, Color, Fill, FontData},
    };

    #[derive(Clone)]
    enum Command {
        Fill(BezPath, Affine, Brush, Fill),
        Rect(Rect, Affine, Brush, Affine, Fill),
        Stroke(BezPath, Affine, Brush, Affine, Stroke),
        Clip(Rect, Affine),
        Pop,
        Append(Arc<Vec<Command>>),
        Glyphs(FontData, f32, Affine, Color, Arc<[Glyph]>),
    }

    /// Viewport-independent vector commands, preprocessed only after the visible crop is known.
    #[derive(Default)]
    pub(crate) struct Scene {
        commands: Arc<Vec<Command>>,
    }
    impl Scene {
        /// Creates an empty retained command list.
        pub(crate) fn new() -> Self {
            Self::default()
        }
        /// Retains a filled path and its canvas transform/brush.
        pub(crate) fn fill(
            &mut self,
            rule: Fill,
            transform: Affine,
            brush: &Brush,
            path: &BezPath,
        ) {
            if let Some(rect) = rectangle_path(path) {
                self.fill_rect(rule, transform, brush, rect);
                return;
            }
            Arc::make_mut(&mut self.commands).push(Command::Fill(
                path.clone(),
                transform,
                brush.clone(),
                rule,
            ));
        }
        /// Retains a stroked path with independently transformed paint.
        pub(crate) fn stroke(
            &mut self,
            stroke: &Stroke,
            transform: Affine,
            brush: &Brush,
            brush_transform: Affine,
            path: BezPath,
        ) {
            if let Some(rect) = straight_stroke(&path, stroke) {
                Arc::make_mut(&mut self.commands).push(Command::Rect(
                    rect,
                    transform,
                    brush.clone(),
                    brush_transform,
                    Fill::NonZero,
                ));
                return;
            }
            Arc::make_mut(&mut self.commands).push(Command::Stroke(
                path,
                transform,
                brush.clone(),
                brush_transform,
                stroke.clone(),
            ));
        }
        /// Retains a filled rectangle with paint in the same local coordinate system.
        pub(crate) fn fill_rect(
            &mut self,
            rule: Fill,
            transform: Affine,
            brush: &Brush,
            rect: Rect,
        ) {
            Arc::make_mut(&mut self.commands).push(Command::Rect(
                rect,
                transform,
                brush.clone(),
                Affine::IDENTITY,
                rule,
            ));
        }
        /// Starts an axis-aligned clip in canvas coordinates.
        pub(crate) fn clip_rect(&mut self, transform: Affine, rect: Rect) {
            Arc::make_mut(&mut self.commands).push(Command::Clip(rect, transform));
        }
        /// Ends the most recent clip.
        pub(crate) fn pop_layer(&mut self) {
            Arc::make_mut(&mut self.commands).push(Command::Pop);
        }
        /// Appends shared retained commands without copying their paths or glyph arrays.
        pub(crate) fn append(&mut self, other: &Self) {
            Arc::make_mut(&mut self.commands).push(Command::Append(Arc::clone(&other.commands)));
        }
        /// Retains a shaped run for Vello GPU's native glyph frontend.
        pub(crate) fn glyphs(
            &mut self,
            font: &FontData,
            size: f32,
            transform: Affine,
            color: Color,
            glyphs: Arc<[Glyph]>,
        ) {
            Arc::make_mut(&mut self.commands).push(Command::Glyphs(
                font.clone(),
                size,
                transform,
                color,
                glyphs,
            ));
        }
        /// Preprocesses the retained commands in physical viewport coordinates.
        pub(crate) fn replay(
            &self,
            target: &mut vello_gpu::Scene,
            resources: &mut vello_gpu::Resources,
            curves: &mut crate::curves::CurveCache,
            transform: Affine,
        ) {
            replay(&self.commands, target, resources, curves, transform);
        }
    }

    // A single closed rectangle is equivalent for both supported winding rules.
    // Compound paths and holes must retain their original path semantics.
    fn rectangle_path(path: &BezPath) -> Option<Rect> {
        let [
            PathEl::MoveTo(a),
            PathEl::LineTo(b),
            PathEl::LineTo(c),
            PathEl::LineTo(d),
            PathEl::ClosePath,
        ] = path.elements()
        else {
            return None;
        };
        let horizontal_first = a.y == b.y && b.x == c.x && c.y == d.y && d.x == a.x;
        let vertical_first = a.x == b.x && b.y == c.y && c.x == d.x && d.y == a.y;
        if horizontal_first || vertical_first {
            Some(Rect::from_points(*a, *c))
        } else {
            None
        }
    }

    // One-segment axis-aligned butt/square strokes are exactly filled rectangles.
    // Rounded caps, curves, joins and multi-segment/dashed paths retain native stroking.
    fn straight_stroke(path: &BezPath, stroke: &Stroke) -> Option<Rect> {
        let [PathEl::MoveTo(a), PathEl::LineTo(b)] = path.elements() else {
            return None;
        };
        if stroke.start_cap != stroke.end_cap
            || stroke.start_cap == Cap::Round
            || !stroke.dash_pattern.is_empty()
            || stroke.width <= 0.0
        {
            return None;
        }
        let half = stroke.width / 2.0;
        let cap = if stroke.start_cap == Cap::Square {
            half
        } else {
            0.0
        };
        if a.y == b.y && a.x != b.x {
            Some(Rect::new(
                a.x.min(b.x) - cap,
                a.y - half,
                a.x.max(b.x) + cap,
                a.y + half,
            ))
        } else if a.x == b.x && a.y != b.y {
            Some(Rect::new(
                a.x - half,
                a.y.min(b.y) - cap,
                a.x + half,
                a.y.max(b.y) + cap,
            ))
        } else {
            None
        }
    }

    fn paint(target: &mut vello_gpu::Scene, brush: &Brush) {
        match brush {
            Brush::Solid(color) => target.set_paint(*color),
            Brush::Gradient(gradient) => target.set_paint(gradient.clone()),
            Brush::Image(_) => {
                unreachable!("The supported canvas API does not create image brushes")
            }
        }
    }
    fn replay(
        commands: &[Command],
        target: &mut vello_gpu::Scene,
        resources: &mut vello_gpu::Resources,
        curves: &mut crate::curves::CurveCache,
        outer: Affine,
    ) {
        for command in commands {
            match command {
                Command::Fill(path, transform, brush, rule) => {
                    target.set_transform(outer * *transform);
                    target.reset_paint_transform();
                    target.set_fill_rule(*rule);
                    paint(target, brush);
                    target.fill_path(path);
                }
                Command::Rect(rect, transform, brush, brush_transform, rule) => {
                    target.set_transform(outer * *transform);
                    target.set_paint_transform(*brush_transform);
                    target.set_fill_rule(*rule);
                    paint(target, brush);
                    target.fill_rect(rect);
                }
                Command::Stroke(path, transform, brush, brush_transform, stroke) => {
                    let transform = outer * *transform;
                    paint(target, brush);
                    if let Some(cached) = curves.resolve(path, stroke, transform) {
                        target.set_transform(cached.transform());
                        target.set_paint_transform(
                            cached.paint_transform(transform * *brush_transform),
                        );
                        target.set_fill_rule(Fill::NonZero);
                        target.fill_path(cached.path());
                    } else {
                        target.set_transform(transform);
                        target.set_paint_transform(*brush_transform);
                        target.set_stroke(stroke.clone());
                        target.stroke_path(path);
                    }
                }
                Command::Clip(rect, transform) => {
                    target.set_transform(outer * *transform);
                    target.push_clip_rect(rect);
                }
                Command::Pop => target.pop_clip(),
                Command::Append(commands) => replay(commands, target, resources, curves, outer),
                Command::Glyphs(font, size, transform, color, glyphs) => {
                    // Text uses nonzero winding independently of preceding path fills.
                    target.set_fill_rule(Fill::NonZero);
                    target.set_transform(outer * *transform);
                    target.reset_paint_transform();
                    target.set_paint(*color);
                    let [a, b, c, d, _, _] = (outer * *transform).as_coeffs();
                    // Cache only positive uniform scales; arbitrary affine text stays vector-native.
                    let atlas = a > 0.0 && a == d && b == 0.0 && c == 0.0;
                    target
                        .glyph_run(resources, font)
                        .font_size(*size)
                        .hint(false)
                        .atlas_cache(atlas)
                        .fill_glyphs(glyphs.iter().map(|g| glifo::Glyph {
                            id: g.id,
                            x: g.x,
                            y: g.y,
                        }))
                        .expect("Vello GPU glyph rendering failed");
                }
            }
        }
    }
}
