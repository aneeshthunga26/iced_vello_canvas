//! The same drawing code runs through Iced's canvas and the experimental Vello canvas.
use iced::widget::{column, container, row, text};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

#[derive(Default)]
struct App {
    zoomed: bool,
    dense: bool,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Zoom,
    Density,
}

fn main() -> iced::Result {
    iced::application(App::default, App::update, App::view)
        .title("Iced / Vello canvas comparison")
        .window_size((1120.0, 640.0))
        .theme(Theme::Light)
        .antialiasing(true)
        .run()
}

impl App {
    fn update(&mut self, message: Message) {
        match message {
            Message::Zoom => self.zoomed = !self.zoomed,
            Message::Density => self.dense = !self.dense,
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let zoom = if self.zoomed { 3.0 } else { 1.0 };
        let count = if self.dense { 10_000 } else { 1_000 };
        let left = column![
            text("Iced canvas · AA enabled").size(20),
            iced::widget::canvas(reference::Drawing { zoom, count })
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .spacing(12)
        .width(Length::Fill);
        let right = column![
            text("Vello Compute canvas · area AA").size(20),
            iced_vello_canvas::canvas(experiment::Drawing { zoom, count })
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .spacing(12)
        .width(Length::Fill);
        container(
            column![
                text(format!(
                    "Zoom {zoom:.0}× · {count} paths · left-click: zoom · right-click: density"
                ))
                .size(14),
                row![left, right].spacing(24).height(Length::Fill),
            ]
            .spacing(20),
        )
        .padding(24)
        .into()
    }
}

// Expand the same program for both namespaces to exercise import-level compatibility.
macro_rules! drawing {
    () => {
        /// Scene parameters shared by both renderer implementations.
        pub(super) struct Drawing {
            /// Magnification in logical canvas coordinates.
            pub(super) zoom: f32,
            /// Number of circle paths in the density fixture.
            pub(super) count: usize,
        }

        /// Retains geometry until a scene parameter or the canvas size changes.
        #[derive(Default)]
        pub(super) struct State {
            cache: canvas::Cache,
            key: std::cell::Cell<(u32, usize)>,
        }

        impl canvas::Program<Message> for Drawing {
            type State = State;

            fn update(
                &self,
                _: &mut State,
                event: &canvas::Event,
                bounds: Rectangle,
                cursor: mouse::Cursor,
            ) -> Option<canvas::Action<Message>> {
                if !cursor.is_over(bounds) {
                    return None;
                }
                match event {
                    canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                        Some(canvas::Action::publish(Message::Zoom).and_capture())
                    }
                    canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                        Some(canvas::Action::publish(Message::Density).and_capture())
                    }
                    _ => None,
                }
            }

            fn mouse_interaction(
                &self,
                _: &State,
                bounds: Rectangle,
                cursor: mouse::Cursor,
            ) -> mouse::Interaction {
                if cursor.is_over(bounds) {
                    mouse::Interaction::Pointer
                } else {
                    mouse::Interaction::default()
                }
            }

            fn draw(
                &self,
                state: &State,
                renderer: &Renderer,
                _: &Theme,
                bounds: Rectangle,
                _: mouse::Cursor,
            ) -> Vec<canvas::Geometry> {
                let key = (self.zoom.to_bits(), self.count);
                if state.key.replace(key) != key {
                    state.cache.clear();
                }
                vec![state.cache.draw(renderer, bounds.size(), |frame| {
                    frame.translate(Vector::new(12.0, 12.0));
                    frame.scale(self.zoom);
                    frame.fill_text(canvas::Text {
                        content: "Antialiasing · café · ffi 0123456789".into(),
                        position: Point::new(0.0, 0.0),
                        color: Color::from_rgb8(24, 24, 27),
                        size: 18.0.into(),
                        shaping: iced::advanced::text::Shaping::Advanced,
                        ..Default::default()
                    });
                    frame.fill_text(canvas::Text {
                        content: "Small text, curves and subpixel strokes".into(),
                        position: Point::new(0.0, 32.0),
                        color: Color::from_rgb8(70, 70, 75),
                        size: 11.0.into(),
                        ..Default::default()
                    });
                    for i in 0..18 {
                        frame.stroke(
                            &canvas::Path::line(
                                Point::new(0.0, 68.0 + i as f32 * 2.5),
                                Point::new(200.0, 82.0 + i as f32 * 2.5),
                            ),
                            canvas::Stroke::default().with_width(0.5 + i as f32 / 18.0),
                        );
                    }
                    frame.with_save(|frame| {
                        frame.translate(Vector::new(280.0, 102.0));
                        frame.rotate(0.35);
                        frame.stroke(
                            &canvas::Path::circle(Point::ORIGIN, 34.0),
                            canvas::Stroke::default().with_width(0.8),
                        );
                        frame.fill(
                            &canvas::Path::rectangle(
                                Point::new(-24.0, -6.0),
                                Size::new(48.0, 12.0),
                            ),
                            Color::from_rgba(0.15, 0.35, 0.8, 0.5),
                        );
                    });
                    frame.with_clip(
                        Rectangle {
                            x: 0.0,
                            y: 145.0,
                            width: 460.0,
                            height: 320.0,
                        },
                        |frame| {
                            for i in 0..self.count {
                                let x = (i % 100) as f32 * 4.7;
                                let y = 145.0 + (i / 100) as f32 * 3.2;
                                let path = canvas::Path::circle(Point::new(x, y), 1.45);
                                frame.fill(
                                    &path,
                                    Color::from_rgba(0.12, 0.3 + (i % 5) as f32 * 0.08, 0.7, 0.65),
                                );
                            }
                        },
                    );
                })]
            }
        }
    };
}

mod reference {
    use super::*;
    use iced::widget::canvas;
    drawing!();
}

mod experiment {
    use super::*;
    use iced_vello_canvas as canvas;
    drawing!();
}
