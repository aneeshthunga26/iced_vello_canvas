//! Animate the same cubic Bézier strokes through Iced and Vello.
use std::time::Duration;

use iced::time::Instant;
use iced::widget::{Space, button, column, container, pick_list, row, text};
use iced::{Alignment, Color, Element, Fill, Point, Rectangle, Renderer, Size, Theme, Vector};
use iced::{mouse, window};

const COUNTS: [usize; 3] = [1_000, 3_000, 10_000];
const PANEL_GAP: u32 = 16;
const FPS_SAMPLE: Duration = Duration::from_millis(500);
const VELLO: &str = if cfg!(feature = "gpu") {
    "Vello GPU"
} else {
    "Vello Compute"
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Both,
    Iced,
    Vello,
}

#[derive(Clone, Copy, Debug)]
enum Message {
    SelectMode(Mode),
    SelectCount(usize),
    Fps(f64),
}

struct App {
    mode: Mode,
    count: usize,
    curves: Vec<Curve>,
    started: Instant,
    fps: Option<f64>,
}

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("Bézier canvas comparison")
        .window(window::Settings {
            size: Size::new(1180.0, 700.0),
            min_size: Some(Size::new(680.0, 380.0)),
            ..Default::default()
        })
        .theme(Theme::Dark)
        .settings(iced::Settings {
            antialiasing: true,
            vsync: false,
            ..Default::default()
        })
        .run()
}

impl App {
    fn new() -> Self {
        Self {
            mode: Mode::Both,
            count: COUNTS[1],
            curves: (0..COUNTS[2]).map(Curve::new).collect(),
            started: Instant::now(),
            fps: None,
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::SelectMode(mode) if self.mode != mode => self.mode = mode,
            Message::SelectCount(count) if self.count != count => self.count = count,
            Message::Fps(fps) => {
                self.fps = Some(fps);
                return;
            }
            _ => return,
        }
        self.started = Instant::now();
        self.fps = None;
    }

    fn view(&self) -> Element<'_, Message> {
        let fps = self
            .fps
            .map_or_else(|| "— FPS".to_owned(), |fps| format!("{fps:.0} FPS"));
        let heading = row![
            text("Bézier curves").size(28),
            Space::new().width(Fill),
            text(fps).size(24),
        ]
        .align_y(Alignment::Center);

        let controls = row![
            self.mode_button(Mode::Both, "Side by side"),
            self.mode_button(Mode::Iced, "Iced only"),
            self.mode_button(Mode::Vello, "Vello only"),
            Space::new().width(Fill),
            text("Curves per canvas").size(14),
            pick_list(COUNTS, Some(self.count), Message::SelectCount).text_size(14),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let scene = Scene {
            curves: &self.curves[..self.count],
            started: self.started,
        };
        let iced_panel = || {
            panel(
                "Iced Canvas",
                iced::widget::canvas(reference::Drawing {
                    scene,
                    report_fps: true,
                })
                .width(Fill)
                .height(Fill)
                .into(),
            )
        };
        let vello_panel = || {
            panel(
                VELLO,
                iced_vello_canvas::canvas(experiment::Drawing {
                    scene,
                    report_fps: self.mode == Mode::Vello,
                })
                .width(Fill)
                .height(Fill)
                .into(),
            )
        };
        let canvases = match self.mode {
            Mode::Both => row![iced_panel().width(Fill), vello_panel().width(Fill),]
                .spacing(PANEL_GAP)
                .height(Fill),
            Mode::Iced => center_panel(iced_panel()),
            Mode::Vello => center_panel(vello_panel()),
        };

        container(column![heading, controls, canvases].spacing(16))
            .padding(20)
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn mode_button<'a>(&self, mode: Mode, label: &'a str) -> button::Button<'a, Message> {
        let selected = self.mode == mode;
        button(text(label).size(14))
            .style(if selected {
                button::primary
            } else {
                button::secondary
            })
            .on_press(Message::SelectMode(mode))
    }
}

fn panel<'a>(
    title: &'static str,
    canvas: Element<'a, Message>,
) -> iced::widget::Column<'a, Message> {
    column![
        text(title).size(16),
        container(canvas)
            .style(container::bordered_box)
            .width(Fill)
            .height(Fill),
    ]
    .spacing(8)
    .height(Fill)
}

fn center_panel<'a>(panel: iced::widget::Column<'a, Message>) -> iced::widget::Row<'a, Message> {
    // The solo panel has the same width as either half of the two-panel view.
    row![
        Space::new().width(iced::Length::FillPortion(1)),
        panel.width(iced::Length::FillPortion(2)),
        Space::new().width(iced::Length::FillPortion(1)),
    ]
    .spacing(PANEL_GAP / 2)
    .height(Fill)
}

struct Curve {
    path: iced::widget::canvas::Path,
    origin: Point,
    velocity: Vector,
    color: Color,
}

impl Curve {
    fn new(index: usize) -> Self {
        const PALETTE: [[u8; 3]; 5] = [
            [115, 177, 255],
            [89, 225, 205],
            [178, 145, 255],
            [255, 155, 193],
            [255, 205, 128],
        ];
        let index = index as u32;
        let seed = f64::from(index);
        let x = (seed * 0.618_033_988_75).fract() as f32;
        let y = (seed * 0.754_877_666_25).fract() as f32;
        let shape = (seed * 0.569_840_290_99).fract() as f32;
        let half_span = 6.0 + 24.0 * shape;
        let bend = 5.0 + 13.0 * x;
        let (sin, cos) = ((y - 0.5) * 2.6).sin_cos();
        let point = |u: f32, v: f32| Point::new(cos * u - sin * v, sin * u + cos * v);
        let path = iced::widget::canvas::Path::new(|builder| {
            builder.move_to(point(-half_span, 0.0));
            builder.bezier_curve_to(
                point(-half_span * 0.6, -bend),
                point(half_span * 0.6, bend),
                point(half_span, 0.0),
            );
        });
        let [red, green, blue] = PALETTE[index as usize % PALETTE.len()];
        Self {
            path,
            origin: Point::new(x, 2.0 * y),
            velocity: Vector::new(0.10 + 0.24 * y, 0.09 + 0.18 * x),
            color: Color::from_rgba8(red, green, blue, 0.56),
        }
    }

    fn translation(&self, size: Size, seconds: f32) -> Vector {
        let position = |origin: f32, speed: f32, extent: f32| {
            let phase = (origin + speed * seconds).rem_euclid(2.0);
            let reflected = 1.0 - (phase - 1.0).abs();
            extent / 2.0 + (reflected - 0.5) * (extent - 72.0).max(0.0)
        };
        Vector::new(
            position(self.origin.x, self.velocity.x, size.width),
            position(self.origin.y, self.velocity.y, size.height),
        )
    }
}

#[derive(Clone, Copy)]
struct Scene<'a> {
    curves: &'a [Curve],
    started: Instant,
}

#[derive(Default)]
struct Animation {
    last_frame: Option<Instant>,
    sample_start: Option<Instant>,
    frame_intervals: u32,
}

impl Animation {
    fn tick(&mut self, now: Instant, started: Instant) -> Option<f64> {
        // Iced can deliver the same redraw timestamp again after a widget message.
        if self.last_frame.replace(now) == Some(now) {
            return None;
        }

        let Some(sample_start) = self.sample_start.filter(|&sample| sample >= started) else {
            self.sample_start = Some(now);
            self.frame_intervals = 0;
            return None;
        };

        self.frame_intervals += 1;
        let elapsed = now.saturating_duration_since(sample_start);
        if elapsed < FPS_SAMPLE {
            return None;
        }
        let fps = f64::from(self.frame_intervals) / elapsed.as_secs_f64();
        self.sample_start = Some(now);
        self.frame_intervals = 0;
        Some(fps)
    }
}

// The two canvas APIs receive the same timestamps, paths, transforms and stroke color.
macro_rules! drawing {
    () => {
        /// Draws the shared animation through this module's canvas API.
        pub(super) struct Drawing<'a> {
            /// The paths and time origin shared by both panels.
            pub(super) scene: Scene<'a>,
            /// Whether this panel reports the window's frame rate.
            pub(super) report_fps: bool,
        }

        impl canvas::Program<Message> for Drawing<'_> {
            type State = Animation;

            fn update(
                &self,
                state: &mut Animation,
                event: &canvas::Event,
                _bounds: Rectangle,
                _cursor: mouse::Cursor,
            ) -> Option<canvas::Action<Message>> {
                let canvas::Event::Window(window::Event::RedrawRequested(now)) = event else {
                    return None;
                };
                if let Some(fps) = state
                    .tick(*now, self.scene.started)
                    .filter(|_| self.report_fps)
                {
                    Some(canvas::Action::publish(Message::Fps(fps)))
                } else {
                    Some(canvas::Action::request_redraw())
                }
            }

            fn draw(
                &self,
                state: &Animation,
                renderer: &Renderer,
                _theme: &Theme,
                bounds: Rectangle,
                _cursor: mouse::Cursor,
            ) -> Vec<canvas::Geometry> {
                let mut frame = canvas::Frame::new(renderer, bounds.size());
                let elapsed = state
                    .last_frame
                    .unwrap_or(self.scene.started)
                    .saturating_duration_since(self.scene.started)
                    .as_secs_f32();

                for curve in self.scene.curves {
                    let translation = curve.translation(bounds.size(), elapsed);
                    frame.with_save(|frame| {
                        frame.translate(translation);
                        frame.stroke(
                            &curve.path,
                            canvas::Stroke::default()
                                .with_width(1.15)
                                .with_color(curve.color),
                        );
                    });
                }

                vec![frame.into_geometry()]
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
