use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use parking_lot::Mutex;

use sasa::{AudioManager, AudioRecorder, Renderer, Recorder};

use macroquad::prelude::*;

use crate::backend;
use crate::{draw_button_colored, TEXT, TEXT_DIM, BTN_GREEN, BTN_GREEN_HOVER};

pub enum LatencyTests {
    RoundTrip(RoundTripTest),
    TapToTone(TapToToneTest),
    Error(String),
}

impl LatencyTests {
    pub fn new(is_tap_to_tone: bool) -> Result<Self, String> {
        if is_tap_to_tone {
            TapToToneTest::new().map(LatencyTests::TapToTone)
        } else {
            RoundTripTest::new().map(LatencyTests::RoundTrip)
        }
    }

    pub fn dummy() -> Self { LatencyTests::Error("Init failed".into()) }

    pub fn render(&mut self) {
        match self {
            LatencyTests::RoundTrip(t) => t.render(),
            LatencyTests::TapToTone(t) => t.render(),
            LatencyTests::Error(e) => {
                draw_text(&format!("Error: {e}"), 20.0, 80.0, 20.0, Color::new(1.0, 0.3, 0.3, 1.0));
            }
        }
    }
}

struct BeepRenderer {
    trigger: Arc<AtomicBool>,
    sample_count: Arc<AtomicU64>,
    remaining: Mutex<u32>,
    frequency: f32,
    phase: f64,
}

impl BeepRenderer {
    fn new(trigger: Arc<AtomicBool>, sample_count: Arc<AtomicU64>, frequency: f32) -> Self {
        Self { trigger, sample_count, remaining: Mutex::new(0), frequency, phase: 0.0 }
    }
}

impl Renderer for BeepRenderer {
    fn alive(&self) -> bool { true }
    fn render_mono(&mut self, sample_rate: u32, data: &mut [f32]) {
        if self.trigger.swap(false, Ordering::Relaxed) {
            *self.remaining.lock() = (sample_rate as f32 * 0.08) as u32;
            self.sample_count.store(0, Ordering::Relaxed);
        }
        let mut rem = *self.remaining.lock();
        let sr = sample_rate as f64;
        for sample in data.iter_mut() {
            if rem > 0 {
                let val = ((self.phase * 2.0 * std::f64::consts::PI).sin() as f32 * 0.7).clamp(-1.0, 1.0);
                *sample = val;
                self.phase = (self.phase + self.frequency as f64 / sr) % 1.0;
                rem -= 1;
            } else { *sample = 0.0; }
        }
        *self.remaining.lock() = rem;
        self.sample_count.fetch_add(data.len() as u64, Ordering::Relaxed);
    }
    fn render_stereo(&mut self, sample_rate: u32, data: &mut [f32]) {
        if self.trigger.swap(false, Ordering::Relaxed) {
            *self.remaining.lock() = (sample_rate as f32 * 0.08) as u32;
            self.sample_count.store(0, Ordering::Relaxed);
        }
        let mut rem = *self.remaining.lock();
        let sr = sample_rate as f64;
        for chunk in data.chunks_exact_mut(2) {
            if rem > 0 {
                let val = ((self.phase * 2.0 * std::f64::consts::PI).sin() as f32 * 0.7).clamp(-1.0, 1.0);
                chunk[0] = val;
                chunk[1] = val;
                self.phase = (self.phase + self.frequency as f64 / sr) % 1.0;
                rem -= 1;
            } else { chunk[0] = 0.0; chunk[1] = 0.0; }
        }
        *self.remaining.lock() = rem;
        self.sample_count.fetch_add(data.len() as u64 / 2, Ordering::Relaxed);
    }
}

struct RingBuffer {
    data: Vec<f32>,
    start_pos: u64,
    capacity: usize,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self { data: Vec::with_capacity(capacity), start_pos: 0, capacity }
    }

    fn push_samples(&mut self, samples: &[f32]) {
        self.data.extend_from_slice(samples);
        if self.data.len() > self.capacity {
            let excess = self.data.len() - self.capacity;
            self.data.drain(..excess);
            self.start_pos += excess as u64;
        }
    }

    #[allow(dead_code)]
    fn total_written(&self) -> u64 { self.start_pos + self.data.len() as u64 }

    fn read_range(&self, from_pos: u64, len: usize) -> Vec<f32> {
        let end = self.start_pos + self.data.len() as u64;
        if from_pos >= end { return vec![]; }
        let offset = if from_pos < self.start_pos { 0 } else { (from_pos - self.start_pos) as usize };
        let available = self.data.len() - offset;
        let count = len.min(available);
        self.data[offset..offset + count].to_vec()
    }
}

struct TapRecorder {
    buffer: Arc<Mutex<RingBuffer>>,
    position: Arc<AtomicU64>,
    sample_rate: Arc<AtomicU32>,
}

impl TapRecorder {
    fn new(buffer: Arc<Mutex<RingBuffer>>, position: Arc<AtomicU64>, sample_rate: Arc<AtomicU32>) -> Self {
        Self { buffer, position, sample_rate }
    }
}

impl Recorder for TapRecorder {
    fn alive(&self) -> bool { true }
    fn record_mono(&mut self, sr: u32, data: &[f32]) {
        self.sample_rate.store(sr, Ordering::Relaxed);
        self.buffer.lock().push_samples(data);
        self.position.fetch_add(data.len() as u64, Ordering::Relaxed);
    }
    fn record_stereo(&mut self, sample_rate: u32, data: &[f32]) {
        let mono: Vec<f32> = data.chunks_exact(2).map(|c| (c[0] + c[1]) * 0.5).collect();
        self.record_mono(sample_rate, &mono);
    }
}

pub struct TapToToneTest {
    manager: Option<AudioManager>,
    recorder: Option<AudioRecorder>,
    trigger: Arc<AtomicBool>,
    sample_count: Arc<AtomicU64>,
    position: Arc<AtomicU64>,
    sample_rate: Arc<AtomicU32>,
    buffer: Arc<Mutex<RingBuffer>>,
    result_ms: Option<f64>,
    measuring: bool,
    trigger_pos: Option<u64>,
    trigger_time: Option<std::time::Instant>,
    phase: AnalysisPhase,
    viz_samples: Vec<f32>,
    viz_slow: Vec<f32>,
    viz_fast: Vec<f32>,
    viz_threshold: Vec<f32>,
    viz_edges: Vec<usize>,
    viz_sr: u32,
    measurement_count: usize,
    latency_sum: f64,
    latency_min: f64,
    latency_max: f64,
}

#[derive(PartialEq)]
enum AnalysisPhase {
    Idle,
    WaitingForBeep,
    Scheduled,
    Done,
}

impl TapToToneTest {
    pub fn new() -> Result<Self, String> {
        let trigger = Arc::new(AtomicBool::new(false));
        let sample_count = Arc::new(AtomicU64::new(0));
        let position = Arc::new(AtomicU64::new(0));
        let sample_rate = Arc::new(AtomicU32::new(48000));

        let backend_out = backend::make_output();
        let mut mgr = AudioManager::new_box(Box::new(backend_out)).map_err(|e| format!("{e}"))?;
        let beep = BeepRenderer::new(trigger.clone(), sample_count.clone(), 1000.0);
        mgr.add_renderer(beep).map_err(|e| format!("{e}"))?;
        mgr.start().map_err(|e| format!("{e}"))?;

        let backend_in = backend::make_input_shared();
        let mut rec = AudioRecorder::new_box(Box::new(backend_in)).map_err(|e| format!("{e}"))?;
        let buf = Arc::new(Mutex::new(RingBuffer::new(48000 * 5)));
        let tap_rec = TapRecorder::new(buf.clone(), position.clone(), sample_rate.clone());
        rec.add_recorder(tap_rec).map_err(|e| format!("{e}"))?;
        rec.start().map_err(|e| format!("{e}"))?;

        Ok(Self {
            manager: Some(mgr), recorder: Some(rec),
            trigger, sample_count, position, sample_rate, buffer: buf,
            result_ms: None, measuring: false, trigger_pos: None,
            trigger_time: None, phase: AnalysisPhase::Idle,
            viz_samples: Vec::new(), viz_slow: Vec::new(), viz_fast: Vec::new(),
            viz_threshold: Vec::new(), viz_edges: Vec::new(), viz_sr: 48000,
            measurement_count: 0, latency_sum: 0.0,
            latency_min: f64::MAX, latency_max: 0.0,
        })
    }

    fn analyze(&mut self) {
        let sr = self.sample_rate.load(Ordering::Relaxed);
        let sr_f = sr as f64;
        if sr == 0 {
            self.phase = AnalysisPhase::Done;
            self.measuring = false;
            return;
        }

        let buf = self.buffer.lock();
        let trigger_pos = self.trigger_pos.unwrap_or(0);
        let pre_samples = (sr_f * 0.2) as usize;
        let post_samples = (sr_f * 1.2) as usize;
        let from = trigger_pos.saturating_sub(pre_samples as u64);
        let total = pre_samples + post_samples;

        let samples = buf.read_range(from, total);
        drop(buf);

        self.viz_sr = sr;
        self.viz_edges.clear();

        if samples.len() < 1024 {
            self.viz_samples = samples;
            self.result_ms = Some(-3.0);
            self.phase = AnalysisPhase::Done;
            self.measuring = false;
            return;
        }

        let hp = high_pass(&samples, sr_f, 0.95);

        let events_from_hp = apply_envelope_and_scan(&hp, sr_f,
            &mut self.viz_fast, &mut self.viz_slow, &mut self.viz_threshold);

        if events_from_hp.len() == 2 {
            self.viz_samples = hp;
            self.viz_edges = events_from_hp;
        } else {
            let avg = average_filter(&hp);
            let events_from_avg = apply_envelope_and_scan(&avg, sr_f,
                &mut self.viz_fast, &mut self.viz_slow, &mut self.viz_threshold);
            if events_from_avg.len() == 2 {
                self.viz_samples = avg;
                self.viz_edges = events_from_avg;
            } else {
                let gentle_hp = high_pass(&samples, sr_f, 0.80);
                let events_from_gentle = apply_envelope_and_scan(&gentle_hp, sr_f,
                    &mut self.viz_fast, &mut self.viz_slow, &mut self.viz_threshold);
                self.viz_samples = gentle_hp;
                self.viz_edges = events_from_gentle;
            }
        }

        if self.viz_edges.len() >= 2 {
            let latency_samples = self.viz_edges[1] - self.viz_edges[0];
            let latency_ms = latency_samples as f64 / sr_f * 1000.0;
            self.result_ms = Some(latency_ms);
            self.measurement_count += 1;
            self.latency_sum += latency_ms;
            if latency_ms < self.latency_min { self.latency_min = latency_ms; }
            if latency_ms > self.latency_max { self.latency_max = latency_ms; }
        } else if self.viz_edges.len() == 1 {
            self.result_ms = Some(-1.0);
        } else {
            self.result_ms = Some(-2.0);
        }

        self.phase = AnalysisPhase::Done;
        self.measuring = false;
    }

    pub fn render(&mut self) {
        let s = crate::scale();
        let left = screen_width() * 0.08;

        draw_text("Tap to Tone Latency Test", left, 60.0 * s, 28.0 * s, TEXT);
        draw_text("Tap screen, press Space, or click button. Beep plays immediately.", left, 95.0 * s, 16.0 * s, TEXT_DIM);
        draw_text("Latency = time from tap sound -> beep sound at microphone.", left, 115.0 * s, 14.0 * s, TEXT_DIM);

        if let Some(ref mut mgr) = self.manager {
            if mgr.consume_broken() { let _ = mgr.recover_if_needed(); }
        }
        if let Some(ref mut rec) = self.recorder {
            if rec.consume_broken() { let _ = rec.recover_if_needed(); }
        }

        if self.phase == AnalysisPhase::WaitingForBeep {
            if let Some(t) = self.trigger_time {
                if t.elapsed().as_secs_f64() > 1.2 {
                    self.phase = AnalysisPhase::Scheduled;
                    self.analyze();
                }
            }
        }

        let cx = screen_width() / 2.0;
        let cy = screen_height() / 2.0 - 90.0 * s;
        let tap_size = 180.0 * s;

        let (mx, my) = mouse_position();
        let hovering = mx >= cx - tap_size / 2.0 && mx <= cx + tap_size / 2.0
            && my >= cy - tap_size / 2.0 && my <= cy + tap_size / 2.0;

        let tap_color = if self.measuring {
            Color::new(0.85, 0.55, 0.10, 1.0)
        } else if hovering {
            Color::new(0.20, 0.65, 0.25, 0.85)
        } else {
            Color::new(0.15, 0.50, 0.20, 0.7)
        };
        draw_circle(cx, cy, tap_size / 2.0, tap_color);

        if self.measuring {
            draw_text("Measuring...", cx - 50.0 * s, cy + 10.0 * s, 22.0 * s, TEXT);
        } else {
            draw_text("TAP HERE", cx - 45.0 * s, cy + 10.0 * s, 24.0 * s, TEXT);
        }

        let btn_x = cx - 80.0 * s;
        let btn_y = cy + tap_size / 2.0 + 10.0 * s;
        if draw_button_colored(btn_x, btn_y, 160.0 * s, 36.0 * s, "Trigger Beep", 18.0 * s, BTN_GREEN, BTN_GREEN_HOVER) {
            self.trigger_measurement();
        }

        let reset_x = cx - 35.0 * s;
        if draw_button_colored(reset_x, btn_y + 40.0 * s, 70.0 * s, 28.0 * s, "Reset", 14.0 * s, Color::new(0.4, 0.4, 0.45, 1.0), Color::new(0.55, 0.55, 0.6, 1.0)) {
            self.measurement_count = 0;
            self.latency_sum = 0.0;
            self.latency_min = f64::MAX;
            self.latency_max = 0.0;
            self.result_ms = None;
            self.viz_samples.clear();
            self.viz_edges.clear();
        }

        if hovering && !self.measuring && is_mouse_button_pressed(MouseButton::Left)
            && mx >= cx - tap_size / 2.0 && mx <= cx + tap_size / 2.0
            && my >= cy - tap_size / 2.0 && my <= cy + tap_size / 2.0
        {
            self.trigger_measurement();
        }

        if !self.measuring && is_key_pressed(KeyCode::Space) {
            self.trigger_measurement();
        }

        if let Some(ms) = self.result_ms {
            let result_y = cy + tap_size / 2.0 + 110.0 * s;
            let fs = 16.0 * s;
            if ms == -3.0 {
                draw_text("Not enough audio captured - try again", cx - 140.0 * s, result_y, fs, Color::new(1.0, 0.5, 0.3, 1.0));
            } else if ms == -2.0 {
                draw_text("No edges detected - try again", cx - 110.0 * s, result_y, fs, Color::new(1.0, 0.5, 0.3, 1.0));
            } else if ms < 0.0 {
                draw_text("Only 1 edge detected - tap harder or use fingernail", cx - 180.0 * s, result_y, fs, Color::new(1.0, 0.5, 0.3, 1.0));
            } else {
                draw_text(&format!("Latency: {ms:.1} ms"), cx - 85.0 * s, result_y, 28.0 * s, Color::new(0.3, 1.0, 0.4, 1.0));
            }
        }

        if self.measurement_count > 0 {
            let avg = self.latency_sum / self.measurement_count as f64;
            let stats_y = cy + tap_size / 2.0 + 130.0 * s;
            draw_text(
                &format!("min: {:.1}  avg: {:.1}  max: {:.1} ms  ({})",
                    self.latency_min, avg, self.latency_max, self.measurement_count),
                cx - 110.0 * s, stats_y, 15.0 * s, TEXT_DIM,
            );
        }

        if !self.viz_samples.is_empty() {
            self.draw_waveform();
        }
    }

    fn draw_waveform(&self) {
        let s = crate::scale();
        let sw = screen_width();
        let margin = 40.0 * s;
        let wf_w = sw - margin * 2.0;
        let wf_h = 120.0 * s;
        let wf_x = margin;
        let wf_y = screen_height() - wf_h - 50.0 * s;

        draw_rectangle(wf_x - 1.0, wf_y - 1.0, wf_w + 2.0, wf_h + 2.0, Color::new(0.2, 0.2, 0.25, 1.0));
        draw_rectangle(wf_x, wf_y, wf_w, wf_h, Color::new(0.05, 0.05, 0.08, 1.0));

        let mid_y = wf_y + wf_h / 2.0;
        draw_line(wf_x, mid_y, wf_x + wf_w, mid_y, 1.0, Color::new(0.25, 0.25, 0.30, 1.0));

        let n = self.viz_samples.len();
        if n < 2 { return; }

        let max_val = self.viz_samples.iter().cloned().fold(0.0f32, f32::max).max(0.001);
        let step = (n as f32 / (wf_w * 2.0) as f32).max(1.0);

        let mut prev_x = wf_x;
        let mut prev_y = mid_y - (self.viz_samples[0] / max_val) * wf_h / 2.0;
        let mut i: f32 = step;
        while (i as usize) < n {
            let idx = i as usize;
            let x = wf_x + (idx as f32 / n as f32) * wf_w;
            let y = mid_y - (self.viz_samples[idx] / max_val) * wf_h / 2.0;
            draw_line(prev_x, prev_y.clamp(wf_y, wf_y + wf_h), x, y.clamp(wf_y, wf_y + wf_h), 1.0, Color::new(0.3, 0.65, 0.9, 1.0));
            prev_x = x;
            prev_y = y;
            i += step;
        }

        if !self.viz_fast.is_empty() {
            let fast_max = self.viz_fast.iter().cloned().fold(0.0f32, f32::max).max(0.001);
            let mut px = wf_x;
            let mut py = mid_y - (self.viz_fast[0] / fast_max) * wf_h / 2.0;
            let mut j: f32 = step;
            while (j as usize) < self.viz_fast.len().min(n) {
                let idx = j as usize;
                let x = wf_x + (idx as f32 / n as f32) * wf_w;
                let y = mid_y - (self.viz_fast[idx] / fast_max) * wf_h / 2.0;
                draw_line(px, py.clamp(wf_y, wf_y + wf_h), x, y.clamp(wf_y, wf_y + wf_h), 1.0, Color::new(0.9, 0.2, 0.2, 0.6));
                px = x; py = y; j += step;
            }
        }

        draw_text("Envelope (blue) | Fast avg (red)", wf_x, wf_y - 16.0 * s, 12.0 * s, TEXT_DIM);

        let labels = ["Tap", "Tone"];
        let colors = [Color::new(1.0, 0.8, 0.2, 0.9), Color::new(0.3, 1.0, 0.3, 0.9)];
        for (k, &edge) in self.viz_edges.iter().enumerate() {
            if k >= 2 { break; }
            let ex = wf_x + (edge as f32 / n as f32) * wf_w;
            draw_line(ex, wf_y, ex, wf_y + wf_h, 2.0, colors[k]);
            let time_ms = edge as f64 / self.viz_sr as f64 * 1000.0;
            let lx = if ex > wf_x + wf_w / 2.0 { ex - 100.0 * s } else { ex + 4.0 * s };
            draw_text(&format!("{}= {:.1}ms", labels[k], time_ms), lx, wf_y - 4.0 * s, 13.0 * s, colors[k]);
        }
    }

    fn trigger_measurement(&mut self) {
        if self.phase != AnalysisPhase::Idle && self.phase != AnalysisPhase::Done { return; }
        self.result_ms = None;
        self.measuring = true;
        self.phase = AnalysisPhase::WaitingForBeep;
        self.viz_samples.clear();
        self.viz_slow.clear();
        self.viz_fast.clear();
        self.viz_threshold.clear();
        self.viz_edges.clear();
        self.trigger_pos = Some(self.position.load(Ordering::Relaxed));
        self.trigger_time = Some(std::time::Instant::now());
        self.sample_count.store(0, Ordering::Relaxed);
        self.trigger.store(true, Ordering::Relaxed);
    }
}

fn high_pass(signal: &[f32], _sample_rate: f64, alpha: f64) -> Vec<f32> {
    let mut out = vec![0.0f32; signal.len()];
    let mut xn1 = 0.0f64;
    let mut yn1 = 0.0f64;
    for i in 0..signal.len() {
        let xn = signal[i] as f64;
        let yn = alpha * (yn1 + xn - xn1);
        out[i] = yn as f32;
        xn1 = xn;
        yn1 = yn;
    }
    out
}

fn average_filter(signal: &[f32]) -> Vec<f32> {
    let n = signal.len();
    if n == 0 { return vec![]; }
    let avg: f64 = signal.iter().map(|&x| x as f64).sum::<f64>() / n as f64;
    let variance: f64 = signal.iter().map(|&x| { let d = x as f64 - avg; d * d }).sum::<f64>() / n as f64;
    let std_dev = variance.sqrt();
    let threshold = std_dev * 1.5;
    signal.iter().map(|&x| {
        let d = (x as f64 - avg).abs();
        if d >= threshold { x } else { 0.0 }
    }).collect()
}

fn apply_envelope_and_scan(buffer: &[f32], sample_rate: f64,
    fast_buf: &mut Vec<f32>, slow_buf: &mut Vec<f32>, threshold_buf: &mut Vec<f32>,
) -> Vec<usize> {
    let n = buffer.len();
    let mut envelope = vec![0.0f32; n];
    let mut prev = 0.0f32;
    for i in 0..n {
        let input = buffer[i].abs();
        let output = if input > prev * 0.995 { input } else { prev * 0.995 };
        prev = output;
        envelope[i] = output;
    }

    let mut events = Vec::new();
    let mut slow: f32 = 0.0;
    let mut fast: f32 = 0.0;
    let edge_threshold: f32 = 0.01;
    let slow_coeff: f32 = 0.01;
    let fast_coeff: f32 = 0.10;
    let mut low_threshold: f32 = edge_threshold;
    let mut armed = true;

    fast_buf.clear();
    slow_buf.clear();
    threshold_buf.clear();

    let skip = (sample_rate * 0.003) as usize;

    for i in 0..n {
        let level = envelope[i];
        slow += (level - slow) * slow_coeff;
        fast += (level - fast) * fast_coeff;
        slow_buf.push(slow);
        fast_buf.push(fast);

        if armed && i >= skip && fast > edge_threshold && fast > 2.0 * slow {
            events.push(i);
            armed = false;
            low_threshold = fast * 0.5;
        }
        threshold_buf.push(low_threshold);

        if fast < low_threshold {
            armed = true;
        }
    }

    events
}

struct PulseRenderer {
    trigger: Arc<AtomicBool>,
    active: Mutex<bool>,
    position: Mutex<usize>,
    pulse: Arc<Vec<f32>>,
}

impl PulseRenderer {
    fn new(trigger: Arc<AtomicBool>, pulse: Arc<Vec<f32>>) -> Self {
        Self { trigger, active: Mutex::new(false), position: Mutex::new(0), pulse }
    }
}

impl Renderer for PulseRenderer {
    fn alive(&self) -> bool { true }
    fn render_mono(&mut self, _sr: u32, data: &mut [f32]) {
        if self.trigger.swap(false, Ordering::Relaxed) {
            *self.active.lock() = true;
            *self.position.lock() = 0;
        }
        let mut active = *self.active.lock();
        let mut pos = *self.position.lock();
        for sample in data.iter_mut() {
            if active && pos < self.pulse.len() {
                *sample = self.pulse[pos];
                pos += 1;
            } else {
                *sample = 0.0;
                active = false;
            }
        }
        *self.position.lock() = pos;
        *self.active.lock() = active;
    }
    fn render_stereo(&mut self, _sr: u32, data: &mut [f32]) {
        if self.trigger.swap(false, Ordering::Relaxed) {
            *self.active.lock() = true;
            *self.position.lock() = 0;
        }
        let mut active = *self.active.lock();
        let mut pos = *self.position.lock();
        for chunk in data.chunks_exact_mut(2) {
            if active && pos < self.pulse.len() {
                chunk[0] = self.pulse[pos];
                chunk[1] = self.pulse[pos];
                pos += 1;
            } else {
                chunk[0] = 0.0; chunk[1] = 0.0;
                active = false;
            }
        }
        *self.position.lock() = pos;
        *self.active.lock() = active;
    }
}

struct RtRecorder {
    buffer: Arc<Mutex<Vec<f32>>>,
    position: Arc<AtomicU64>,
}

impl RtRecorder {
    fn new(buffer: Arc<Mutex<Vec<f32>>>, position: Arc<AtomicU64>) -> Self {
        Self { buffer, position }
    }
}

impl Recorder for RtRecorder {
    fn alive(&self) -> bool { true }
    fn record_mono(&mut self, _sr: u32, data: &[f32]) {
        self.buffer.lock().extend_from_slice(data);
        self.position.fetch_add(data.len() as u64, Ordering::Relaxed);
    }
    fn record_stereo(&mut self, sr: u32, data: &[f32]) {
        let mono: Vec<f32> = data.chunks_exact(2).map(|c| (c[0] + c[1]) * 0.5).collect();
        self.record_mono(sr, &mono);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum RtState {
    Idle,
    Background,   // measuring background noise, output silent
    Playing,      // pulse playing, recording
    Collecting,   // pulse finished, still collecting tail
    Done,
}

pub struct RoundTripTest {
    result_ms: Option<f64>,
    state: RtState,
    trigger: Arc<AtomicBool>,
    state_start: Option<std::time::Instant>,
    manager: Option<AudioManager>,
    recorder: Option<AudioRecorder>,
    record_buf: Arc<Mutex<Vec<f32>>>,
    rec_position: Arc<AtomicU64>,
    pulse: Arc<Vec<f32>>,
    trigger_pos: Option<u64>,
    pulse_len: usize,
    sample_rate: u32,
    pub error_text: Option<String>,
    background_rms: f64,
    viz_data: Vec<f32>,
}

impl RoundTripTest {
    pub fn new() -> Result<Self, String> {
        let sr = 48000;
        let pulse_len = sr as usize / 4;
        let pulse = Arc::new(generate_white_noise_pulse(pulse_len));

        let trigger = Arc::new(AtomicBool::new(false));

        let backend_out = backend::make_output();
        let mut mgr = AudioManager::new_box(Box::new(backend_out)).map_err(|e| format!("{e}"))?;
        mgr.add_renderer(PulseRenderer::new(trigger.clone(), pulse.clone())).map_err(|e| format!("{e}"))?;
        mgr.start().map_err(|e| format!("{e}"))?;

        let backend_in = backend::make_input();
        let mut rec = AudioRecorder::new_box(Box::new(backend_in)).map_err(|e| format!("{e}"))?;
        let buf = Arc::new(Mutex::new(Vec::with_capacity(sr as usize * 3)));
        let rec_pos = Arc::new(AtomicU64::new(0));
        rec.add_recorder(RtRecorder::new(buf.clone(), rec_pos.clone())).map_err(|e| format!("{e}"))?;
        rec.start().map_err(|e| format!("{e}"))?;

        Ok(Self {
            result_ms: None, state: RtState::Idle,
            trigger, state_start: None,
            manager: Some(mgr), recorder: Some(rec),
            record_buf: buf, rec_position: rec_pos,
            pulse, trigger_pos: None, pulse_len,
            sample_rate: sr, error_text: None,
            background_rms: 0.0,
            viz_data: Vec::new(),
        })
    }

    pub fn render(&mut self) {
        let s = crate::scale();
        let left = screen_width() * 0.08;

        draw_text("Round Trip Latency Test", left, 60.0 * s, 28.0 * s, TEXT);
        draw_text("Measures speaker -> microphone latency.", left, 95.0 * s, 16.0 * s, TEXT_DIM);

        if let Some(ref e) = self.error_text {
            draw_text(e, left, 118.0 * s, 14.0 * s, Color::new(1.0, 0.3, 0.3, 1.0));
        }

        if let Some(ref mut mgr) = self.manager {
            if mgr.consume_broken() { let _ = mgr.recover_if_needed(); }
        }
        if let Some(ref mut rec) = self.recorder {
            if rec.consume_broken() { let _ = rec.recover_if_needed(); }
        }

        self.tick_state();

        let cx = screen_width() / 2.0;
        let cy = screen_height() / 2.0;

        let (label, color) = match self.state {
            RtState::Idle => ("Measure", BTN_GREEN),
            RtState::Background => ("Measuring background...", Color::new(0.4, 0.4, 0.6, 1.0)),
            RtState::Playing => ("Playing pulse...", Color::new(0.7, 0.5, 0.1, 1.0)),
            RtState::Collecting => ("Collecting...", Color::new(0.5, 0.3, 0.7, 1.0)),
            RtState::Done => ("Measure", BTN_GREEN),
        };
        let hover = if self.state == RtState::Idle || self.state == RtState::Done { BTN_GREEN_HOVER } else { color };

        if draw_button_colored(cx - 100.0 * s, cy - 20.0 * s, 200.0 * s, 48.0 * s, label, 22.0 * s, color, hover) {
            if self.state == RtState::Idle || self.state == RtState::Done {
                self.start_measurement();
            }
        }

        if let Some(ms) = self.result_ms {
            let result_y = cy + 80.0 * s;
            if ms < 0.0 {
                let msg = self.error_text.as_deref().unwrap_or("Detection failed - ensure speaker is near microphone");
                draw_text(msg, cx - 180.0 * s, result_y, 18.0 * s, Color::new(1.0, 0.5, 0.3, 1.0));
            } else {
                draw_text(&format!("Round-trip latency: {ms:.2} ms"), cx - 130.0 * s, result_y, 26.0 * s, Color::new(0.3, 1.0, 0.4, 1.0));
            }
        }

        if !self.viz_data.is_empty() {
            self.draw_waveform();
        }
    }

    fn draw_waveform(&self) {
        let s = crate::scale();
        let sw = screen_width();
        let margin = 40.0 * s;
        let wf_w = sw - margin * 2.0;
        let wf_h = 100.0 * s;
        let wf_x = margin;
        let wf_y = screen_height() - wf_h - 40.0 * s;

        draw_rectangle(wf_x - 1.0, wf_y - 1.0, wf_w + 2.0, wf_h + 2.0, Color::new(0.2, 0.2, 0.25, 1.0));
        draw_rectangle(wf_x, wf_y, wf_w, wf_h, Color::new(0.05, 0.05, 0.08, 1.0));

        let mid_y = wf_y + wf_h / 2.0;
        draw_line(wf_x, mid_y, wf_x + wf_w, mid_y, 1.0, Color::new(0.25, 0.25, 0.30, 1.0));

        let n = self.viz_data.len();
        if n < 2 { return; }

        let max_val = self.viz_data.iter().cloned().fold(0.0f32, f32::max).max(0.001);
        let step = (n as f32 / (wf_w * 2.0) as f32).max(1.0);

        let mut prev_x = wf_x;
        let mut prev_y = mid_y - (self.viz_data[0] / max_val) * wf_h / 2.0;
        let mut i: f32 = step;
        while (i as usize) < n {
            let idx = i as usize;
            let x = wf_x + (idx as f32 / n as f32) * wf_w;
            let y = mid_y - (self.viz_data[idx] / max_val) * wf_h / 2.0;
            draw_line(prev_x, prev_y.clamp(wf_y, wf_y + wf_h), x, y.clamp(wf_y, wf_y + wf_h), 1.0, Color::new(0.3, 0.65, 0.9, 1.0));
            prev_x = x;
            prev_y = y;
            i += step;
        }

        if let Some(ms) = self.result_ms {
            if ms >= 0.0 {
                let latency_samples = (ms * self.sample_rate as f64 / 1000.0) as usize;
                if latency_samples < n {
                    let lx = wf_x + (latency_samples as f32 / n as f32) * wf_w;
                    draw_line(lx, wf_y, lx, wf_y + wf_h, 2.0, Color::new(0.3, 1.0, 0.4, 0.9));
                    draw_text(&format!("peak={:.1}ms", ms), lx + 4.0 * s, wf_y + 14.0 * s, 12.0 * s, Color::new(0.3, 1.0, 0.4, 1.0));
                }
            }
        }

        draw_text("Recorded audio", wf_x, wf_y - 16.0 * s, 12.0 * s, TEXT_DIM);
    }

    fn tick_state(&mut self) {
        match self.state {
            RtState::Background => {
                if self.state_start.unwrap().elapsed().as_secs_f32() > 0.5 {
                    let rec = self.record_buf.lock();
                    if rec.len() >= 48000 / 4 {
                        let sum_sq: f64 = rec.iter().map(|&x| (x as f64).powi(2)).sum();
                        self.background_rms = (sum_sq / rec.len() as f64).sqrt();
                    }
                    drop(rec);
                    self.record_buf.lock().clear();
                    self.trigger_pos = Some(self.rec_position.load(Ordering::Relaxed));
                    self.trigger.store(true, Ordering::Relaxed);
                    self.state_start = Some(std::time::Instant::now());
                    self.state = RtState::Playing;
                }
            }
            RtState::Playing => {
                if self.state_start.unwrap().elapsed().as_secs_f32() > 0.6 {
                    self.state_start = Some(std::time::Instant::now());
                    self.state = RtState::Collecting;
                }
            }
            RtState::Collecting => {
                if self.state_start.unwrap().elapsed().as_secs_f32() > 0.4 {
                    self.analyze();
                }
            }
            _ => {}
        }
    }

    fn start_measurement(&mut self) {
        self.result_ms = None;
        self.error_text = None;
        self.background_rms = 0.0;
        self.viz_data.clear();
        self.record_buf.lock().clear();
        self.state_start = Some(std::time::Instant::now());
        self.state = RtState::Background;
    }

    fn analyze(&mut self) {
        self.state = RtState::Done;

        let recorded = self.record_buf.lock().clone();
        self.viz_data = recorded.clone();
        if recorded.len() < self.pulse_len / 2 {
            self.result_ms = Some(-1.0);
            self.error_text = Some("Not enough audio captured - check volume".into());
            return;
        }

        let mut normalized = recorded.clone();
        let max_val = normalized.iter().cloned().fold(0.0f32, f32::max).max(0.0001);
        for x in &mut normalized { *x /= max_val; }

        let lag = measure_latency_from_pulse(&normalized, &self.pulse);
        if lag < 0 {
            self.result_ms = Some(-1.0);
            if self.background_rms < 0.001 {
                self.error_text = Some("Background too quiet - check mic".into());
            } else {
                self.error_text = Some("No correlation peak found".into());
            }
            return;
        }

        let latency_ms = lag as f64 / self.sample_rate as f64 * 1000.0;
        self.result_ms = Some(latency_ms);
    }
}

fn generate_white_noise_pulse(length: usize) -> Vec<f32> {
    let pattern: [u8; 10] = [1, 0, 0, 1, 1, 0, 0, 0, 1, 0];
    let block = length / pattern.len();
    let mut pulse = vec![0.0f32; length];

    for (b, &on) in pattern.iter().enumerate() {
        if on == 0 { continue; }
        let start = b * block;
        let end = ((b + 1) * block).min(length);

        let mut seed: u64 = 12345 + b as u64;
        for i in start..end {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let r = ((seed >> 16) & 0x7FFF) as f32 / 32768.0;
            pulse[i] = (r * 2.0 - 1.0) * 0.5;
        }
    }

    pulse
}

fn measure_latency_from_pulse(recorded: &[f32], pulse: &[f32]) -> i32 {
    if recorded.len() < pulse.len() { return -1; }

    let coarse_stride = 16usize;
    let coarse_limit = recorded.len() - pulse.len();
    let mut best_corr = -2.0f64;
    let mut best_lag = 0usize;

    for lag in (0..coarse_limit).step_by(coarse_stride) {
        let (sum_prod, sum_sq) = correlation_at(recorded, pulse, lag, coarse_stride);
        if sum_sq >= 1e-9 {
            let corr = 2.0 * sum_prod / sum_sq;
            if corr > best_corr {
                best_corr = corr;
                best_lag = lag;
            }
        }
    }

    if best_corr < 0.15 { return -1; }

    let fine_window = coarse_stride * 8;
    let fine_start = best_lag.saturating_sub(fine_window / 2);
    let fine_limit = (recorded.len() - pulse.len()).min(fine_start + fine_window);
    best_corr = -2.0f64;

    for lag in fine_start..fine_limit {
        let (sum_prod, sum_sq) = correlation_at(recorded, pulse, lag, 1);
        if sum_sq >= 1e-9 {
            let corr = 2.0 * sum_prod / sum_sq;
            if corr > best_corr {
                best_corr = corr;
                best_lag = lag;
            }
        }
    }

    if best_corr < 0.15 { return -1; }
    best_lag as i32
}

fn correlation_at(recorded: &[f32], pulse: &[f32], lag: usize, stride: usize) -> (f64, f64) {
    let mut sum_prod = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut i = 0;
    while i < pulse.len() {
        let a = recorded[lag + i] as f64;
        let b = pulse[i] as f64;
        sum_prod += a * b;
        sum_sq += a * a + b * b;
        i += stride;
    }
    (sum_prod, sum_sq)
}
