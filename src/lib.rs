mod output;
mod input;
mod latency;
mod backend;

use macroquad::prelude::*;

pub fn scale() -> f32 { screen_height() / 720.0 }

const BG: Color = Color::new(0.08, 0.08, 0.14, 1.0);

fn init_logging() {
    #[cfg(target_os = "android")]
    {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Debug)
                .with_tag("audio-tester"),
        );
        std::panic::set_hook(Box::new(|info| {
            log::error!("Panic: {}", info);
        }));
    }
}
const BTN: Color = Color::new(0.18, 0.45, 0.75, 1.0);
const BTN_HOVER: Color = Color::new(0.25, 0.55, 0.88, 1.0);
const BTN_GREEN: Color = Color::new(0.15, 0.65, 0.25, 1.0);
const BTN_GREEN_HOVER: Color = Color::new(0.20, 0.75, 0.30, 1.0);
const BTN_RED: Color = Color::new(0.75, 0.18, 0.18, 1.0);
const BTN_RED_HOVER: Color = Color::new(0.88, 0.25, 0.25, 1.0);
const TEXT: Color = Color::new(0.92, 0.92, 0.92, 1.0);
const TEXT_DIM: Color = Color::new(0.55, 0.55, 0.60, 1.0);
const METER_BG: Color = Color::new(0.15, 0.15, 0.20, 1.0);
const METER_GREEN: Color = Color::new(0.15, 0.80, 0.20, 1.0);
const METER_YELLOW: Color = Color::new(0.90, 0.85, 0.10, 1.0);
const METER_RED: Color = Color::new(0.90, 0.15, 0.15, 1.0);

fn draw_button(x: f32, y: f32, w: f32, h: f32, label: &str, font_size: f32) -> bool {
    let (mx, my) = mouse_position();
    let hovered = mx >= x && mx <= x + w && my >= y && my <= y + h;
    let color = if hovered { BTN_HOVER } else { BTN };
    draw_rectangle(x, y, w, h, color);
    let dim = measure_text(label, None, font_size as u16, 1.0);
    draw_text(label, x + (w - dim.width) / 2.0, y + (h + dim.height) / 2.0, font_size, TEXT);
    hovered && is_mouse_button_pressed(MouseButton::Left)
}

fn draw_button_colored(x: f32, y: f32, w: f32, h: f32, label: &str, font_size: f32, base: Color, hover: Color) -> bool {
    let (mx, my) = mouse_position();
    let hovered = mx >= x && mx <= x + w && my >= y && my <= y + h;
    draw_rectangle(x, y, w, h, if hovered { hover } else { base });
    let dim = measure_text(label, None, font_size as u16, 1.0);
    draw_text(label, x + (w - dim.width) / 2.0, y + (h + dim.height) / 2.0, font_size, TEXT);
    hovered && is_mouse_button_pressed(MouseButton::Left)
}

fn draw_slider(x: f32, y: f32, w: f32, h: f32, value: f32, label: &str, show_value: bool) -> f32 {
    draw_rectangle(x, y, w, h, Color::new(0.25, 0.25, 0.30, 1.0));
    draw_rectangle(x, y, w * value, h, Color::new(0.35, 0.60, 0.90, 1.0));
    let val_text = if show_value { format!("{label}: {:.0}", value * 100.0) } else { format!("{label}: {:.2}", value) };
    draw_text(&val_text, x, y - 4.0, 16.0, TEXT_DIM);

    let (mx, my) = mouse_position();
    if is_mouse_button_down(MouseButton::Left)
        && mx >= x && mx <= x + w && my >= y - 10.0 && my <= y + h + 10.0
    {
        ((mx - x) / w).clamp(0.0, 1.0)
    } else {
        value
    }
}

fn log_slider_val(normalized: f32, min: f32, max: f32) -> f32 {
    let log_min = min.ln();
    let log_max = max.ln();
    (log_min + normalized * (log_max - log_min)).exp()
}

fn log_slider_to_norm(val: f32, min: f32, max: f32) -> f32 {
    let log_min = min.ln();
    let log_max = max.ln();
    ((val.ln() - log_min) / (log_max - log_min)).clamp(0.0, 1.0)
}

enum Screen {
    Menu,
    OutputTest,
    InputTest,
    RoundTripLatency,
    TapToTone,
}

#[unsafe(no_mangle)]
pub extern "C" fn quad_main() {
    macroquad::Window::from_config(Conf {
            window_title: "Audio Tester".into(),
            ..Default::default()
        }, async {
        the_main().await
    });
}

async fn the_main() {
    init_logging();

    let mut screen = Screen::Menu;
    let mut output_test: Option<output::OutputTest> = None;
    let mut input_test: Option<input::InputTest> = None;
    let mut latency_test: Option<latency::LatencyTests> = None;

    loop {
        clear_background(BG);

        match &mut screen {
            Screen::Menu => {
                let _ = output_test.take();
                let _ = input_test.take();
                let _ = latency_test.take();
                if let Some(next) = draw_menu() {
                    screen = next;
                }
            }
            Screen::OutputTest => {
                if output_test.is_none() {
                    let _ = input_test.take();
                    let _ = latency_test.take();
                    output_test = Some(output::OutputTest::new().unwrap_or_else(|e| {
                        log::error!("Output test init error: {e}");
                        output::OutputTest::dummy()
                    }));
                }
                if let Some(ref mut t) = output_test {
                    if draw_back_button() { screen = Screen::Menu; } else { t.render(); }
                }
            }
            Screen::InputTest => {
                if input_test.is_none() {
                    let _ = output_test.take();
                    let _ = latency_test.take();
                    input_test = Some(input::InputTest::new().unwrap_or_else(|e| {
                        log::error!("Input test init error: {e}");
                        input::InputTest::dummy()
                    }));
                }
                if let Some(ref mut t) = input_test {
                    if draw_back_button() { screen = Screen::Menu; } else { t.render(); }
                }
            }
            Screen::RoundTripLatency | Screen::TapToTone => {
                if latency_test.is_none() {
                    let _ = output_test.take();
                    let _ = input_test.take();
                    let tp = matches!(screen, Screen::TapToTone);
                    latency_test = Some(latency::LatencyTests::new(tp).unwrap_or_else(|e| {
                        log::error!("Latency test init error: {e}");
                        latency::LatencyTests::dummy()
                    }));
                }
                if let Some(ref mut t) = latency_test {
                    if draw_back_button() { screen = Screen::Menu; } else { t.render(); }
                }
            }
        }

        next_frame().await;
    }
}

fn draw_back_button() -> bool {
    let s = scale();
    draw_button_colored(12.0 * s, 12.0 * s, 90.0 * s, 34.0 * s, "< Back", 22.0 * s, Color::new(0.3, 0.3, 0.35, 1.0), Color::new(0.45, 0.45, 0.50, 1.0))
}

fn draw_menu() -> Option<Screen> {
    let s = scale();
    let sw = screen_width();
    let sh = screen_height();
    let btn_w = 320.0 * s;
    let btn_h = 52.0 * s;
    let gap = 66.0 * s;
    let start_y = sh / 2.0 - 140.0 * s;
    let x = sw / 2.0 - btn_w / 2.0;

    let fs = 44.0 * s;
    let title = "Audio Tester";
    let title_dim = measure_text(title, None, fs as u16, 1.0);
    draw_text(title, sw / 2.0 - title_dim.width / 2.0, start_y - 50.0 * s, fs, TEXT);

    if draw_button(x, start_y, btn_w, btn_h, "Output Test", 26.0 * s) { return Some(Screen::OutputTest); }
    if draw_button(x, start_y + gap, btn_w, btn_h, "Input Test", 26.0 * s) { return Some(Screen::InputTest); }
    if draw_button(x, start_y + gap * 2.0, btn_w, btn_h, "Round Trip Latency", 22.0 * s) { return Some(Screen::RoundTripLatency); }
    if draw_button(x, start_y + gap * 3.0, btn_w, btn_h, "Tap to Tone Latency", 22.0 * s) { return Some(Screen::TapToTone); }
    None
}
