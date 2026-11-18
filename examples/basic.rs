use iced::{Color, Element, Length, Rectangle, Renderer, Theme, mouse};
use iced_vello_canvas as canvas;

struct Dot;

impl canvas::Program<()> for Dot {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let dot = canvas::Path::circle(frame.center(), 32.0);
        frame.fill(&dot, Color::from_rgb(0.16, 0.38, 0.85));
        vec![frame.into_geometry()]
    }
}

fn view(_state: &()) -> Element<'_, ()> {
    canvas::canvas(Dot)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn main() -> iced::Result {
    iced::run(|_: &mut (), ()| {}, view)
}
