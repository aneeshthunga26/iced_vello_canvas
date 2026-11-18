//! Animate the same cubic Bézier strokes through Iced and Vello.
use std::time::Duration;

use iced::time::Instant;
use iced::widget::{Space, button, column, container, pick_list, row, text};
use iced::{Alignment, Color, Element, Fill, Point, Rectangle, Renderer, Size, Theme, Vector};
use iced::{mouse, window};

const COUNTS: [usize; 3] = [1_000, 3_000, 10_000];
const FPS_REFRESH: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Iced,
    VelloCompute,
}

impl Mode {
    const AVAILABLE: [Self; 2] = [Self::Iced, Self::VelloCompute];

    fn label(self) -> &'static str {
        match self {
            Self::Iced => "Iced",
            Self::VelloCompute => "Vello Compute",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Message {
    SelectMode(Mode),
    SelectCount(usize),
    Fps(Option<f64>),
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
            mode: Mode::Iced,
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
                self.fps = fps;
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
            .map_or_else(|| "— avg FPS".to_owned(), |fps| format!("{fps:.0} avg FPS"));
        let heading = row![
            text("Bézier curves").size(28),
            Space::new().width(Fill),
            text(fps).size(24),
        ]
        .align_y(Alignment::Center);

        let controls = row![
            row(Mode::AVAILABLE
                .iter()
                .map(|&mode| self.mode_button(mode).into()))
            .spacing(8),
            Space::new().width(Fill),
            text("Curves").size(14),
            pick_list(COUNTS, Some(self.count), Message::SelectCount).text_size(14),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let scene = Scene {
            curves: &self.curves[..self.count],
            started: self.started,
        };
        let canvas: Element<'_, Message> = match self.mode {
            Mode::Iced => iced::widget::canvas(reference::Drawing { scene })
                .width(Fill)
                .height(Fill)
                .into(),
            Mode::VelloCompute => iced_vello_canvas::canvas(compute::Drawing { scene })
                .width(Fill)
                .height(Fill)
                .into(),
        };
        let canvas = container(canvas)
            .style(container::bordered_box)
            .width(Fill)
            .height(Fill);

        container(column![heading, controls, canvas].spacing(16))
            .padding(20)
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn mode_button(&self, mode: Mode) -> button::Button<'static, Message> {
        let selected = self.mode == mode;
        button(text(mode.label()).size(14))
            .style(if selected {
                button::primary
            } else {
                button::secondary
            })
            .on_press(Message::SelectMode(mode))
    }
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

struct FpsMeasurement {
    viewport: Size,
    first_frame: Instant,
    last_report: Instant,
    frame_intervals: u64,
}

#[derive(Default)]
struct Animation {
    last_frame: Option<Instant>,
    measurement: Option<FpsMeasurement>,
}

impl Animation {
    fn tick(&mut self, now: Instant, started: Instant, viewport: Size) -> Option<Message> {
        // Iced can deliver the same redraw timestamp again after a widget message.
        if self.last_frame.replace(now) == Some(now) {
            return None;
        }

        let Some(measurement) = self.measurement.as_mut().filter(|measurement| {
            measurement.first_frame >= started && measurement.viewport == viewport
        }) else {
            self.measurement = Some(FpsMeasurement {
                viewport,
                first_frame: now,
                last_report: now,
                frame_intervals: 0,
            });
            return Some(Message::Fps(None));
        };

        measurement.frame_intervals += 1;
        if now - measurement.last_report < FPS_REFRESH {
            return None;
        }
        measurement.last_report = now;
        let elapsed = (now - measurement.first_frame).as_secs_f64();
        Some(Message::Fps(Some(
            measurement.frame_intervals as f64 / elapsed,
        )))
    }
}

// All renderers receive the same timestamps, paths, transforms and stroke color.
macro_rules! drawing {
    () => {
        /// Draws the shared animation through this module's canvas API.
        pub(super) struct Drawing<'a> {
            /// The paths and time origin shared by both renderers.
            pub(super) scene: Scene<'a>,
        }

        impl canvas::Program<Message> for Drawing<'_> {
            type State = Animation;

            fn update(
                &self,
                state: &mut Animation,
                event: &canvas::Event,
                bounds: Rectangle,
                _cursor: mouse::Cursor,
            ) -> Option<canvas::Action<Message>> {
                let canvas::Event::Window(window::Event::RedrawRequested(now)) = event else {
                    return None;
                };
                if let Some(message) = state.tick(*now, self.scene.started, bounds.size()) {
                    Some(canvas::Action::publish(message))
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

mod compute {
    use super::*;
    use iced_vello_canvas as canvas;

    drawing!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn averages_all_elapsed_intervals_and_resets_for_a_new_workload() {
        let mut animation = Animation::default();
        let start = Instant::now();
        let size = Size::new(600.0, 400.0);
        let at = |ms| start + Duration::from_millis(ms);

        assert!(matches!(
            animation.tick(start, start, size),
            Some(Message::Fps(None))
        ));
        assert!(animation.tick(at(200), start, size).is_none());
        assert!(matches!(
            animation.tick(at(600), start, size),
            Some(Message::Fps(Some(fps))) if (fps - 2.0 / 0.6).abs() < 1e-9
        ));
        assert!(animation.tick(at(600), start, size).is_none());
        assert!(matches!(
            animation.tick(at(1200), start, size),
            Some(Message::Fps(Some(fps))) if (fps - 2.5).abs() < 1e-9
        ));

        let restarted_at = at(1300);
        assert!(matches!(
            animation.tick(restarted_at, restarted_at, size),
            Some(Message::Fps(None))
        ));
        assert!(matches!(
            animation.tick(at(1900), restarted_at, size),
            Some(Message::Fps(Some(fps))) if (fps - 1.0 / 0.6).abs() < 1e-9
        ));
        let new_size = Size::new(800.0, 400.0);
        assert!(matches!(
            animation.tick(at(2000), restarted_at, new_size),
            Some(Message::Fps(None))
        ));
        assert!(matches!(
            animation.tick(at(2500), restarted_at, new_size),
            Some(Message::Fps(Some(2.0)))
        ));
    }
}
