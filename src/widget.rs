//! Canvas widget lifecycle adapted from Iced 0.14 (MIT; see LICENSE).
use std::{
    marker::PhantomData,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_CANVAS: AtomicU64 = AtomicU64::new(1);

struct State<S> {
    program: S,
    id: u64,
}

use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, renderer,
    widget::tree::{self, Tree},
};
use iced::{Element, Event, Length, Rectangle, Size, Vector, event, mouse, window};

use crate::{Action, Geometry};

/// The state and logic of a [`Canvas`].
///
/// A [`Program`] can mutate internal state and produce messages for an
/// application.
///
/// [`Canvas`]: crate::Canvas
pub trait Program<Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    /// The internal state mutated by the [`Program`].
    type State: Default + 'static;

    /// Updates the [`State`](Self::State) of the [`Program`].
    ///
    /// When a [`Program`] is used in a [`Canvas`], the runtime will call this
    /// method for each [`Event`].
    ///
    /// This method can optionally return an [`Action`] to either notify an
    /// application of any meaningful interactions, capture the event, or
    /// request a redraw.
    ///
    /// By default, this method does and returns nothing.
    ///
    /// [`Canvas`]: crate::Canvas
    fn update(
        &self,
        _state: &mut Self::State,
        _event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        None
    }

    /// Draws the state of the [`Program`], producing a bunch of [`Geometry`].
    ///
    /// [`Geometry`] can be easily generated with a [`Frame`] or stored in a
    /// [`Cache`].
    ///
    /// [`Geometry`]: crate::Geometry
    /// [`Frame`]: crate::Frame
    /// [`Cache`]: crate::Cache
    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry>;

    /// Returns the current mouse interaction of the [`Program`].
    ///
    /// The interaction returned will be in effect even if the cursor position
    /// is out of bounds of the program's [`Canvas`].
    ///
    /// [`Canvas`]: crate::Canvas
    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::default()
    }
}

impl<Message, Theme, Renderer, T> Program<Message, Theme, Renderer> for &T
where
    T: Program<Message, Theme, Renderer>,
{
    type State = T::State;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        T::update(self, state, event, bounds, cursor)
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        T::draw(self, state, renderer, theme, bounds, cursor)
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        T::mouse_interaction(self, state, bounds, cursor)
    }
}

/// A canvas widget that composites Vello geometry through Iced’s GPU primitive hook.
#[derive(Debug)]
pub struct Canvas<P, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Renderer: iced_wgpu::primitive::Renderer,
    P: Program<Message, Theme, Renderer>,
{
    width: Length,
    height: Length,
    program: P,
    message_: PhantomData<Message>,
    theme_: PhantomData<Theme>,
    renderer_: PhantomData<Renderer>,
    last_mouse_interaction: Option<mouse::Interaction>,
}

impl<P, Message, Theme, Renderer> Canvas<P, Message, Theme, Renderer>
where
    P: Program<Message, Theme, Renderer>,
    Renderer: iced_wgpu::primitive::Renderer,
{
    const DEFAULT_SIZE: f32 = 100.0;

    /// Creates a 100×100 logical-pixel canvas; widget state starts at `P::State::default()`.
    pub fn new(program: P) -> Self {
        Canvas {
            width: Length::Fixed(Self::DEFAULT_SIZE),
            height: Length::Fixed(Self::DEFAULT_SIZE),
            program,
            message_: PhantomData,
            theme_: PhantomData,
            renderer_: PhantomData,
            last_mouse_interaction: None,
        }
    }

    /// Sets the requested layout width; fixed lengths are measured in logical pixels.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the requested layout height; fixed lengths are measured in logical pixels.
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

impl<P, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Canvas<P, Message, Theme, Renderer>
where
    Renderer: iced_wgpu::primitive::Renderer,
    P: Program<Message, Theme, Renderer>,
{
    fn tag(&self) -> tree::Tag {
        struct Tag<T>(T);
        tree::Tag::of::<Tag<P::State>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            program: P::State::default(),
            id: NEXT_CANVAS.fetch_add(1, Ordering::Relaxed),
        })
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        let state = &mut tree.state.downcast_mut::<State<P::State>>().program;
        let is_redraw_request =
            matches!(event, Event::Window(window::Event::RedrawRequested(_now)),);

        if let Some(action) = self.program.update(state, event, bounds, cursor) {
            let (message, redraw_request, event_status) = action.into_inner();

            shell.request_redraw_at(redraw_request);

            if let Some(message) = message {
                shell.publish(message);
            }

            if event_status == event::Status::Captured {
                shell.capture_event();
            }
        }

        if shell.redraw_request() != window::RedrawRequest::NextFrame {
            let mouse_interaction =
                self.mouse_interaction(tree, layout, cursor, viewport, renderer);

            if is_redraw_request {
                self.last_mouse_interaction = Some(mouse_interaction);
            } else if self
                .last_mouse_interaction
                .is_some_and(|last_mouse_interaction| last_mouse_interaction != mouse_interaction)
            {
                shell.request_redraw();
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        let state = &tree.state.downcast_ref::<State<P::State>>().program;

        self.program.mouse_interaction(state, bounds, cursor)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        if bounds.width < 1.0 || bounds.height < 1.0 {
            return;
        }

        let Some(visible) = bounds.intersection(viewport) else {
            return;
        };
        if visible.width < 1.0 || visible.height < 1.0 {
            return;
        }
        let clip = Rectangle {
            x: visible.x - bounds.x,
            y: visible.y - bounds.y,
            ..visible
        };
        let state = &tree.state.downcast_ref::<State<P::State>>().program;

        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            let layers = self.program.draw(state, renderer, theme, bounds, cursor);

            let owner = tree.state.downcast_ref::<State<P::State>>().id;
            for (index, geometry) in layers.into_iter().enumerate() {
                if geometry.empty {
                    continue;
                }
                if geometry
                    .retained
                    .is_some_and(|(_, content)| content.intersection(&clip).is_none())
                {
                    continue;
                }
                renderer.draw_primitive(
                    clip,
                    crate::primitive::Primitive::layer(owner, index, geometry, clip),
                );
            }
        });
    }
}

impl<'a, P, Message, Theme, Renderer> From<Canvas<P, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a + iced_wgpu::primitive::Renderer,
    P: 'a + Program<Message, Theme, Renderer>,
{
    fn from(canvas: Canvas<P, Message, Theme, Renderer>) -> Element<'a, Message, Theme, Renderer> {
        Element::new(canvas)
    }
}
