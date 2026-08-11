use std::sync::Arc;
use parking_lot::Mutex;
use sasa::{AudioManager, Renderer};
use macroquad::prelude::*;

use crate::backend;
use crate::{draw_slider, log_slider_val, log_slider_to_norm, draw_button_colored, TEXT, TEXT_DIM, BTN_GREEN, BTN_GREEN_HOVER, BTN_RED, BTN_RED_HOVER};

#[derive(Clone, Copy, PartialEq)]
enum Waveform {
    Sine,
    Square,
    Sawtooth,
    Triangle,
    WhiteNoise,
    Sweep,
}

struct ToneParams {
    frequency: Mutex<f64>,
    amplitude: Mutex<f32>,
    waveform: Mutex<Waveform>,
    active: Mutex<bool>,
}

struct ToneRenderer {
    phase: f64,
    sweep_phase: f64,
    sweep_dir: f64,
    sample_rate: u32,
    params: Arc<ToneParams>,
}

impl ToneRenderer {
    fn generate(&mut self, data: &mut [f32], is_stereo: bool) {
        if !*self.params.active.lock() {
            data.fill(0.0);
            return;
        }
        let freq = *self.params.frequency.lock();
        let amp = *self.params.amplitude.lock();
        let wf = *self.params.waveform.lock();
        let sr = self.sample_rate as f64;
        let step = if wf == Waveform::Sweep {
            self.sweep_phase += self.sweep_dir * 50.0 / sr;
            if self.sweep_phase >= 8000.0 { self.sweep_dir = -1.0; }
            if self.sweep_phase <= 50.0 { self.sweep_dir = 1.0; }
            self.sweep_phase.clamp(50.0, 8000.0) / sr
        } else {
            freq / sr
        };

        let chunk_size = if is_stereo { 2 } else { 1 };
        for chunk in data.chunks_exact_mut(chunk_size) {
            let val = match wf {
                Waveform::Sine => (self.phase * 2.0 * std::f64::consts::PI).sin() as f32,
                Waveform::Square => if (self.phase * 2.0 * std::f64::consts::PI).sin() >= 0.0 { 1.0 } else { -1.0 },
                Waveform::Sawtooth => ((self.phase % 1.0) * 2.0 - 1.0) as f32,
                Waveform::Triangle => ((self.phase % 1.0) * 4.0 - 1.0).abs().mul_add(2.0, -1.0) as f32,
                Waveform::WhiteNoise => macroquad::rand::rand() as f32 / u32::MAX as f32 * 2.0 - 1.0,
                Waveform::Sweep => (self.phase * 2.0 * std::f64::consts::PI).sin() as f32,
            };
            let sample = (val * amp).clamp(-1.0, 1.0);
            chunk[0] = sample;
            if is_stereo {
                chunk[1] = sample;
            }
            self.phase = (self.phase + step) % 1.0;
        }
    }
}

impl Renderer for ToneRenderer {
    fn alive(&self) -> bool { true }
    fn render_mono(&mut self, sample_rate: u32, data: &mut [f32]) {
        self.sample_rate = sample_rate;
        self.generate(data, false);
    }
    fn render_stereo(&mut self, sample_rate: u32, data: &mut [f32]) {
        self.sample_rate = sample_rate;
        self.generate(data, true);
    }
}

enum OutputState {
    Running(AudioManager),
    Error(String),
}

pub struct OutputTest {
    state: OutputState,
    params: Arc<ToneParams>,
    freq_norm: f32,
    amp_norm: f32,
    waveform_idx: usize,
    stream_info: String,
}

impl OutputTest {
    pub fn new() -> Result<Self, String> {
        let params = Arc::new(ToneParams {
            frequency: Mutex::new(440.0),
            amplitude: Mutex::new(0.5),
            waveform: Mutex::new(Waveform::Sine),
            active: Mutex::new(true),
        });

        let backend = backend::make_output();
        let mut mgr = AudioManager::new_box(Box::new(backend)).map_err(|e| format!("{e}"))?;
        let renderer = ToneRenderer {
            phase: 0.0, sweep_phase: 100.0, sweep_dir: 1.0, sample_rate: 48000,
            params: params.clone(),
        };
        mgr.add_renderer(renderer).map_err(|e| format!("{e}"))?;
        let stream_info = {
            let text = mgr.stream_info().to_string();
            eprintln!("[OutputTest] stream info:\n{text}");
            text
        };

        Ok(Self {
            state: OutputState::Running(mgr),
            params,
            freq_norm: log_slider_to_norm(440.0, 20.0, 8000.0),
            amp_norm: 0.5,
            waveform_idx: 0,
            stream_info,
        })
    }

    pub fn dummy() -> Self {
        Self {
            state: OutputState::Error("Init failed".into()),
            params: Arc::new(ToneParams {
                frequency: Mutex::new(440.0), amplitude: Mutex::new(0.5),
                waveform: Mutex::new(Waveform::Sine), active: Mutex::new(false),
            }),
            freq_norm: 0.5, amp_norm: 0.5, waveform_idx: 0,
            stream_info: String::new(),
        }
    }

    pub fn render(&mut self) {
        let s = crate::scale();
        let sw = screen_width();
        let left = sw * 0.08;
        let right = sw * 0.55;

        draw_text("Output Test", left, 60.0 * s, 32.0 * s, TEXT);

        if let OutputState::Error(e) = &self.state {
            draw_text(&format!("Error: {e}"), left, 120.0 * s, 20.0 * s, Color::new(1.0, 0.3, 0.3, 1.0));
            return;
        }
        if let OutputState::Running(mgr) = &mut self.state {
            if mgr.consume_broken() {
                let _ = mgr.recover_if_needed();
            }
            let latency_ms = mgr.estimate_latency();
            draw_text(&format!("Est. latency: {latency_ms:.4}"), right, 60.0 * s, 18.0 * s, TEXT_DIM);
        }
        if !self.stream_info.is_empty() {
            let lines: Vec<&str> = self.stream_info.lines().collect();
            let fs = 13.0 * s;
            let lh = fs + 3.0 * s;
            let mut info_y = screen_height() - 66.0 * s - lines.len() as f32 * lh;
            for line in lines {
                let col = if line.contains("frame_size_in_callback") { TEXT } else { TEXT_DIM };
                draw_text(line, left, info_y, fs, col);
                info_y += lh;
            }
        }
        let active = *self.params.active.lock();
        let y_btn = screen_height() - 60.0 * s;
        if draw_button_colored(left, y_btn, 100.0 * s, 36.0 * s, if active { "Stop" } else { "Start" }, 20.0 * s,
            if active { BTN_RED } else { BTN_GREEN },
            if active { BTN_RED_HOVER } else { BTN_GREEN_HOVER },
        ) {
            *self.params.active.lock() = !active;
        }

        let y = 100.0 * s;
        let w = 280.0 * s;

        // draw_text("", left, y + 16.0 * s, 18.0 * s, TEXT_DIM);
        self.freq_norm = draw_slider(left, y + 24.0 * s, w, 14.0 * s, self.freq_norm, "", false);
        let freq = log_slider_val(self.freq_norm, 20.0, 8000.0) as f64;
        *self.params.frequency.lock() = freq;
        draw_text(&format!("Frequency: {freq:.0} Hz"), left + w + 12.0 * s, y + 30.0 * s, 16.0 * s, TEXT);

        self.amp_norm = draw_slider(left, y + 60.0 * s, w, 14.0 * s, self.amp_norm, "Volume", false);
        *self.params.amplitude.lock() = self.amp_norm;
        draw_text(&format!("{:.0}%", self.amp_norm * 100.0), left + w + 12.0 * s, y + 66.0 * s, 16.0 * s, TEXT);

        let waveforms = ["Sine", "Square", "Saw", "Triangle", "Noise", "Sweep"];
        let wf_y = y + 110.0 * s;
        draw_text("Waveform", left, wf_y, 18.0 * s, TEXT_DIM);
        for (i, name) in waveforms.iter().enumerate() {
            let bx = left + i as f32 * 76.0 * s;
            let sel = i == self.waveform_idx;
            if draw_button_colored(bx, wf_y + 8.0 * s, 66.0 * s, 30.0 * s, name, 14.0 * s,
                if sel { BTN_GREEN } else { Color::new(0.25, 0.25, 0.30, 1.0) },
                if sel { BTN_GREEN_HOVER } else { Color::new(0.35, 0.35, 0.40, 1.0) },
            ) {
                self.waveform_idx = i;
                let wf = match i {
                    0 => Waveform::Sine, 1 => Waveform::Square, 2 => Waveform::Sawtooth,
                    3 => Waveform::Triangle, 4 => Waveform::WhiteNoise, _ => Waveform::Sweep,
                };
                *self.params.waveform.lock() = wf;
            }
        }
    }
}
