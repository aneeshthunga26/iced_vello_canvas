//! CPU scene caching with Iced's size/explicit-clear invalidation policy.
use std::cell::RefCell;

use iced::{Rectangle, Size};

use crate::{Frame, Geometry, RendererBackend};

/// Retains an encoded scene until its logical bounds change or [`Self::clear`] is called.
///
/// Renderer raster caches also include physical size, so changing DPI rerasterizes the scene.
#[derive(Debug, Default)]
pub struct Cache {
    backend: RendererBackend,
    entry: RefCell<Option<(Rectangle, Geometry)>>,
}

impl Cache {
    /// Creates an empty scene cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty cache whose frames use the selected backend.
    pub fn with_backend(backend: RendererBackend) -> Self {
        Self {
            backend,
            entry: RefCell::new(None),
        }
    }

    /// Invalidates the scene; the next draw calls its closure again.
    pub fn clear(&self) {
        self.entry.borrow_mut().take();
    }

    /// Encodes once per size, returning inexpensive shared geometry on cache hits.
    ///
    /// Theme, content and application transforms must be invalidated explicitly with `clear`.
    pub fn draw<R>(&self, renderer: &R, size: Size, draw: impl FnOnce(&mut Frame)) -> Geometry {
        self.draw_with_bounds(renderer, Rectangle::with_size(size), draw)
    }

    /// Encodes a scene clipped to explicit canvas bounds, invalidating when any bound changes.
    pub fn draw_with_bounds<R>(
        &self,
        renderer: &R,
        bounds: Rectangle,
        draw: impl FnOnce(&mut Frame),
    ) -> Geometry {
        let mut entry = self.entry.borrow_mut();
        if let Some((cached_bounds, geometry)) = entry.as_ref()
            && *cached_bounds == bounds
        {
            return geometry.clone();
        }
        let mut frame = Frame::with_bounds_and_backend(renderer, bounds, self.backend);
        draw(&mut frame);
        let geometry = frame.into_geometry();
        *entry = Some((bounds, geometry.clone()));
        geometry
    }
}
