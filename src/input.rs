use sasa::{AudioRecorder, Record};
use macroquad::prelude::*;

use crate::backend;
use crate::{draw_button_colored, TEXT, TEXT_DIM, BTN_GREEN, BTN_GREEN_HOVER, BTN_RED, BTN_RED_HOVER, METER_BG, METER_GREEN, METER_YELLOW, METER_RED};

enum InputState {
    Running(AudioRecorder, Record),
    Error(String),
}

pub struct InputTest {
    state: InputState,
    peak_l: f32,
    peak_r: f32,
    hold_l: f32,
    hold_r: f32,
    hold_timer: f32,
    active: bool,
}

impl InputTest {
    pub fn new() -> Result<Self, String> {
        let backend = backend::make_input();
        let mut recorder = AudioRecorder::new_box(Box::new(backend)).map_err(|e| format!("{e}"))?;
        let record = recorder.create(Some(8192)).map_err(|e| format!("{e}"))?;
        recorder.start().map_err(|e| format!("{e}"))?;

        Ok(Self {
            state: InputState::Running(recorder, record),
            peak_l: 0.0, peak_r: 0.0, hold_l: 0.0, hold_r: 0.0, hold_timer: 0.0,
            active: true,
        })
    }

    pub fn dummy() -> Self {
        Self {
            state: InputState::Error("Init failed".into()),
            peak_l: 0.0, peak_r: 0.0, hold_l: 0.0, hold_r: 0.0, hold_timer: 0.0,
            active: false,
        }
    }

    fn poll_levels(&mut self) {
        match &mut self.state {
            InputState::Running(recorder, record) => {
                if recorder.consume_broken() {
                    let _ = recorder.recover_if_needed();
                }
                let avail = record.available();
                if avail >= 256 {
                    let mut buf = vec![0.0f32; avail.min(4096)];
                    let n = record.read(&mut buf);
                    let mut max_l = 0.0f32;
                    let mut max_r = 0.0f32;
                    for (i, s) in buf[..n].iter().enumerate() {
                        let abs = s.abs();
                        if i % 2 == 0 { max_l = max_l.max(abs); } else { max_r = max_r.max(abs); }
                    }
                    let smoothing = 0.3;
                    self.peak_l = self.peak_l * (1.0 - smoothing) + max_l * smoothing;
                    self.peak_r = self.peak_r * (1.0 - smoothing) + max_r * smoothing;
                }
            }
            InputState::Error(_) => {}
        }

        let hold_decay = 0.97;
        if self.peak_l > self.hold_l { self.hold_l = self.peak_l; self.hold_timer = 60.0; }
        if self.peak_r > self.hold_r { self.hold_r = self.peak_r; self.hold_timer = 60.0; }
        if self.hold_timer > 0.0 { self.hold_timer -= 1.0; }
        else {
            self.hold_l *= hold_decay;
            self.hold_r *= hold_decay;
        }
    }

    pub fn render(&mut self) {
        let s = crate::scale();
        self.poll_levels();

        let sw = screen_width();
        let left = sw * 0.1;

        draw_text("Input Test", left, 80.0 * s, 32.0 * s, TEXT);

        match &self.state {
            InputState::Error(e) => {
                draw_text(&format!("Error: {e}"), left, 100.0 * s, 20.0 * s, Color::new(1.0, 0.3, 0.3, 1.0));
                return;
            }
            InputState::Running(recorder, _) => {
                let latency_ms = recorder.estimate_latency();
                draw_text(&format!("Est. latency: {latency_ms:.4}"), left, 100.0 * s, 18.0 * s, TEXT_DIM);
            }
        }

        let y_btn = screen_height() - 60.0 * s;
        if draw_button_colored(left, y_btn, 100.0 * s, 36.0 * s, if self.active { "Pause" } else { "Resume" }, 18.0 * s,
            if self.active { BTN_RED } else { BTN_GREEN },
            if self.active { BTN_RED_HOVER } else { BTN_GREEN_HOVER },
        ) {
            self.active = !self.active;
        }

        let meter_x = left + 60.0 * s;
        let meter_w = 40.0 * s;
        let meter_h = 300.0 * s;
        let meter_y = 130.0 * s;
        let gap = 80.0 * s;

        draw_text("L", meter_x + 12.0 * s, meter_y - 8.0 * s, 18.0 * s, TEXT_DIM);
        draw_meter(meter_x, meter_y, meter_w, meter_h, self.peak_l, self.hold_l);

        draw_text("R", meter_x + gap + 12.0 * s, meter_y - 8.0 * s, 18.0 * s, TEXT_DIM);
        draw_meter(meter_x + gap, meter_y, meter_w, meter_h, self.peak_r, self.hold_r);

        let db_l = if self.peak_l > 0.0001 { 20.0 * self.peak_l.log10() } else { -80.0 };
        let db_r = if self.peak_r > 0.0001 { 20.0 * self.peak_r.log10() } else { -80.0 };
        draw_text(&format!("L: {db_l:.1} dB"), meter_x, meter_y + meter_h + 16.0 * s, 16.0 * s, TEXT);
        draw_text(&format!("R: {db_r:.1} dB"), meter_x + gap, meter_y + meter_h + 16.0 * s, 16.0 * s, TEXT);
    }
}

fn draw_meter(x: f32, y: f32, w: f32, h: f32, peak: f32, hold: f32) {
    draw_rectangle(x, y, w, h, METER_BG);

    let level = peak.clamp(0.0, 1.0);
    let fill_h = h * level;
    let color = if level > 0.85 { METER_RED } else if level > 0.6 { METER_YELLOW } else { METER_GREEN };
    draw_rectangle(x, y + h - fill_h, w, fill_h, color);

    let hold_y = y + h - h * hold.clamp(0.0, 1.0);
    draw_rectangle(x, hold_y, w, 2.0, Color::new(1.0, 1.0, 1.0, 0.7));
}
