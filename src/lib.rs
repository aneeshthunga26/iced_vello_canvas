//! A Vello-backed canvas widget for native Iced 0.14 applications.
//!
//! The widget implements a subset of Iced's canvas API and uses the application's
//! existing WGPU renderer. The default `gpu` feature uses Vello's strip renderer. See [the README](https://github.com/aneeshthunga26/iced_vello_canvas)
//! for supported operations and platform features.
//!
//! # Example

#![doc = concat!("```no_run\n", include_str!("../examples/basic.rs"), "\n```")]

mod backend;
mod cache;
#[cfg(feature = "gpu")]
mod curves;
mod frame;
mod primitive;
mod scene;
mod text;
mod widget;

pub use cache::Cache;
pub use frame::{Frame, Geometry};
pub use widget::{Canvas, Program};

pub use iced::widget::canvas::{
    Action, Event, Fill, Gradient, Group, LineCap, LineDash, LineJoin, Path, Stroke, Style, Text,
    fill, gradient, path, stroke,
};

/// Creates a Vello canvas with Iced's default theme and renderer.
pub fn canvas<P, Message>(program: P) -> Canvas<P, Message>
where
    P: Program<Message>,
{
    Canvas::new(program)
}
