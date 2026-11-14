//! Sound: effects and music, all synthesized on the fly (there are no audio files).
//!
//! * [`Mixer`] is plain code that renders stereo samples. It owns every playing
//!   voice, the rocket engine drone and the generative music, so it can be tested
//!   without a sound card.
//! * [`Audio`] opens the default output device with cpal and runs a `Mixer` on the
//!   audio thread. The game talks to it through a channel ([`Cmd`]), so nothing on
//!   the audio thread ever waits for the game. If there is no device the game simply
//!   runs silently.

use std::f32::consts::TAU;
use std::sync::mpsc::{channel, Receiver, Sender};

use crate::util::Rng;

/// One-shot sound effects.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sfx {
    Thud,
    Smash,
    Boom,
    BlackHoleMerge,
    Supernova,
    Crash,
    LiftOff,
    Land,
    Pop,
    Whoosh,
    Step,
    Click,
}

/// Messages from the game to the audio thread.
#[derive(Clone, Copy, Debug)]
pub enum Cmd {
    /// `gain` 0..1, `pan` -1 (left) .. 1 (right).
    Play { sfx: Sfx, gain: f32, pan: f32 },
    /// Rocket engine level, 0..1 (0 = off).
    Engine(f32),
    Volumes(Volumes),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volumes {
    pub master: f32,
    pub effects: f32,
    pub music: f32,
    pub muted: bool,
}

impl Default for Volumes {
    fn default() -> Self {
        Volumes { master: 0.8, effects: 0.8, music: 0.5, muted: false }
    }
}

// ── Building blocks ────────────────────────────────────────────────────────

/// One-pole low-pass filter.
#[derive(Default, Clone, Copy)]
struct LowPass {
    y: f32,
}

impl LowPass {
    fn run(&mut self, x: f32, cutoff: f32, sr: f32) -> f32 {
        let a = 1.0 - (-TAU * cutoff.clamp(10.0, sr * 0.45) / sr).exp();
        self.y += a * (x - self.y);
        self.y
    }
}

/// Exponential sweep from `a` to `b` as `t` goes 0..1 (pitches and cutoffs sound even this way).
fn sweep(a: f32, b: f32, t: f32) -> f32 {
    a * (b / a).powf(t.clamp(0.0, 1.0))
}

/// Equal-power pan: (left gain, right gain).
fn pan_gains(pan: f32) -> (f32, f32) {
    let a = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (a.cos(), a.sin())
}

fn midi_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

// ── Effects ────────────────────────────────────────────────────────────────

/// Recipe for a percussive effect: filtered noise plus a pitched "thump", under a
/// fast-attack, exponential-decay envelope. Every effect is one of these.
#[derive(Clone, Copy, Debug)]
struct Recipe {
    dur: f32,
    attack: f32,
    noise: f32,
    /// Noise low-pass cutoff at the start and end (Hz).
    cut: (f32, f32),
    tone: f32,
    /// Tone pitch at the start and end (Hz).
    pitch: (f32, f32),
    /// Envelope decay rate (higher dies away sooner).
    decay: f32,
}

fn recipe(sfx: Sfx) -> Recipe {
    let r = |dur, attack, noise, cut, tone, pitch, decay| Recipe { dur, attack, noise, cut, tone, pitch, decay };
    match sfx {
        Sfx::Thud => r(0.8, 0.008, 0.5, (700.0, 80.0), 1.0, (75.0, 35.0), 5.0),
        Sfx::Smash => r(2.6, 0.012, 0.9, (1200.0, 60.0), 1.0, (60.0, 24.0), 2.2),
        Sfx::Boom => r(3.2, 0.02, 0.9, (1000.0, 50.0), 1.0, (55.0, 22.0), 1.6),
        Sfx::BlackHoleMerge => r(4.0, 0.25, 0.5, (400.0, 40.0), 1.0, (55.0, 18.0), 1.2),
        Sfx::Supernova => r(6.0, 0.04, 1.0, (1500.0, 40.0), 1.0, (48.0, 18.0), 0.9),
        Sfx::Crash => r(1.8, 0.01, 0.9, (1400.0, 90.0), 1.0, (80.0, 30.0), 2.6),
        Sfx::LiftOff => r(1.4, 0.03, 0.9, (2500.0, 400.0), 0.5, (60.0, 90.0), 2.5),
        Sfx::Land => r(0.35, 0.002, 0.5, (900.0, 120.0), 0.8, (90.0, 50.0), 10.0),
        Sfx::Pop => r(0.25, 0.002, 0.08, (3000.0, 3000.0), 0.8, (300.0, 900.0), 14.0),
        Sfx::Whoosh => r(0.9, 0.25, 0.8, (300.0, 4000.0), 0.0, (100.0, 100.0), 3.5),
        Sfx::Step => r(0.12, 0.002, 0.7, (900.0, 300.0), 0.3, (110.0, 70.0), 30.0),
        Sfx::Click => r(0.05, 0.001, 0.0, (1000.0, 1000.0), 0.6, (1500.0, 1100.0), 60.0),
    }
}

struct Voice {
    r: Recipe,
    t: f32,
    gain: f32,
    pan: (f32, f32),
    phase: f32,
    lp1: LowPass,
    lp2: LowPass,
}

impl Voice {
    fn new(sfx: Sfx, gain: f32, pan: f32) -> Voice {
        Voice { r: recipe(sfx), t: 0.0, gain, pan: pan_gains(pan), phase: 0.0, lp1: LowPass::default(), lp2: LowPass::default() }
    }

    fn done(&self) -> bool {
        self.t >= self.r.dur
    }

    fn sample(&mut self, noise: f32, sr: f32) -> f32 {
        let r = &self.r;
        let u = self.t / r.dur;
        // Fade the last 10% to zero so nothing ends with a click.
        let env = (self.t / r.attack).min(1.0) * (-self.t * r.decay).exp() * ((1.0 - u) * 10.0).min(1.0);
        let cut = sweep(r.cut.0, r.cut.1, u);
        let n = self.lp2.run(self.lp1.run(noise, cut, sr), cut, sr);
        self.phase = (self.phase + sweep(r.pitch.0, r.pitch.1, u) / sr).fract();
        let tone = (self.phase * TAU).sin();
        self.t += 1.0 / sr;
        (n * r.noise * 2.0 + tone * r.tone) * env * self.gain
    }
}

// ── Engine ─────────────────────────────────────────────────────────────────

/// The rocket's roar: brown noise and a low throb that follow the throttle.
#[derive(Default)]
struct Engine {
    target: f32,
    level: f32,
    brown: f32,
    lp: LowPass,
    phase: f32,
}

impl Engine {
    fn sample(&mut self, white: f32, sr: f32) -> f32 {
        // ~60 ms smoothing so throttle changes swell rather than jump.
        self.level += (self.target - self.level) * (1.0 - (-1.0 / (0.06 * sr)).exp());
        if self.level < 1e-4 {
            return 0.0;
        }
        self.brown = (self.brown + white * 0.05) * 0.995;
        let roar = self.lp.run(self.brown, 120.0 + 700.0 * self.level, sr);
        self.phase = (self.phase + (38.0 + 22.0 * self.level) / sr).fract();
        let throb = (self.phase * TAU).sin() * 0.25;
        (roar * 1.2 + throb) * self.level
    }
}

// ── Music ──────────────────────────────────────────────────────────────────

/// A slow, spacey four-chord loop (Cmaj9, Am9, Fmaj9, G6/9) as MIDI notes.
const CHORDS: [[f32; 5]; 4] = [
    [48.0, 55.0, 64.0, 71.0, 74.0],
    [45.0, 52.0, 60.0, 67.0, 71.0],
    [41.0, 48.0, 57.0, 64.0, 67.0],
    [43.0, 50.0, 59.0, 64.0, 69.0],
];
const CHORD_SECONDS: f32 = 10.0;
/// C major pentatonic, for the bells.
const BELL_NOTES: [f32; 8] = [60.0, 62.0, 64.0, 67.0, 69.0, 72.0, 74.0, 76.0];

struct PadNote {
    freq: f32,
    phases: [f32; 2],
    level: f32,
    /// Fading in (true) or out.
    rising: bool,
    pan: (f32, f32),
}

struct Bell {
    freq: f32,
    t: f32,
    phases: [f32; 3],
    gain: f32,
    pan: (f32, f32),
}

/// Generative ambient music: a soft pad that drifts through [`CHORDS`], bell notes
/// now and then, and a stereo echo for space. It never repeats exactly.
struct Music {
    pads: Vec<PadNote>,
    bells: Vec<Bell>,
    chord: usize,
    chord_t: f32,
    next_bell: f32,
    lfo: f32,
    delay: Vec<(f32, f32)>,
    delay_at: usize,
    started: bool,
}

impl Music {
    fn new(sr: f32) -> Music {
        Music {
            pads: Vec::new(),
            bells: Vec::new(),
            chord: 0,
            chord_t: 0.0,
            next_bell: 8.0,
            lfo: 0.0,
            delay: vec![(0.0, 0.0); (sr * 0.42) as usize],
            delay_at: 0,
            started: false,
        }
    }

    fn start_chord(&mut self, rng: &Rng) {
        for p in &mut self.pads {
            p.rising = false;
        }
        for (i, n) in CHORDS[self.chord].iter().enumerate() {
            let pan = (i as f32 / 4.0) * 1.2 - 0.6;
            self.pads.push(PadNote { freq: midi_hz(*n), phases: [rng.f32(), rng.f32()], level: 0.0, rising: true, pan: pan_gains(pan) });
        }
    }

    fn sample(&mut self, rng: &Rng, sr: f32) -> (f32, f32) {
        let dt = 1.0 / sr;
        if !self.started {
            self.started = true;
            self.start_chord(rng);
        }
        self.chord_t += dt;
        if self.chord_t >= CHORD_SECONDS {
            self.chord_t = 0.0;
            self.chord = (self.chord + 1) % CHORDS.len();
            self.start_chord(rng);
        }
        self.next_bell -= dt;
        if self.next_bell <= 0.0 {
            self.next_bell = 7.0 + rng.f32() * 9.0;
            let freq = midi_hz(BELL_NOTES[(rng.f32() * BELL_NOTES.len() as f32) as usize % BELL_NOTES.len()]);
            self.bells.push(Bell { freq, t: 0.0, phases: [0.0; 3], gain: 0.025 + rng.f32() * 0.02, pan: pan_gains(rng.f32() * 1.6 - 0.8) });
        }
        self.lfo = (self.lfo + 0.13 * dt).fract();
        let wobble = 1.0 + (self.lfo * TAU).sin() * 0.002;

        let (mut l, mut r) = (0.0, 0.0);
        // Pads: 3 s fade in, 4 s fade out, two slightly detuned sines each.
        let (rise, fall) = (dt / 3.0, dt / 4.0);
        for p in &mut self.pads {
            p.level = if p.rising { (p.level + rise).min(1.0) } else { (p.level - fall).max(0.0) };
            let mut s = 0.0;
            for (k, ph) in p.phases.iter_mut().enumerate() {
                let detune = if k == 0 { 0.998 } else { 1.002 };
                *ph = (*ph + p.freq * detune * wobble * dt).fract();
                s += (*ph * TAU).sin() + 0.15 * (*ph * TAU * 2.0).sin();
            }
            let v = s * p.level * p.level * 0.035;
            l += v * p.pan.0;
            r += v * p.pan.1;
        }
        self.pads.retain(|p| p.rising || p.level > 0.0);
        // Bells, the higher partials fading first.
        for b in &mut self.bells {
            b.t += dt;
            let mut s = 0.0;
            // A soft mallet: near-harmonic partials, no bright clang on top.
            for (k, (mult, amp, decay)) in [(1.0, 1.0, 0.9), (2.0, 0.25, 1.8), (3.0, 0.08, 3.0)].into_iter().enumerate() {
                b.phases[k] = (b.phases[k] + b.freq * mult * dt).fract();
                s += (b.phases[k] * TAU).sin() * amp * (-b.t * decay).exp();
            }
            let v = s * b.gain * (b.t / 0.04).min(1.0);
            l += v * b.pan.0;
            r += v * b.pan.1;
        }
        self.bells.retain(|b| b.t < 5.0);
        // Ping-pong echo.
        let (dl, dr) = self.delay[self.delay_at];
        self.delay[self.delay_at] = (l + dr * 0.38, r + dl * 0.38);
        self.delay_at = (self.delay_at + 1) % self.delay.len();
        (l + dl * 0.5, r + dr * 0.5)
    }
}

// ── Mixer ──────────────────────────────────────────────────────────────────

/// At most this many effects play at once; the oldest make way for new ones.
const MAX_VOICES: usize = 24;

pub struct Mixer {
    sr: f32,
    voices: Vec<Voice>,
    engine: Engine,
    music: Music,
    rng: Rng,
    /// Volumes are smoothed so moving a slider (or muting) never clicks.
    vol: Volumes,
    gains: (f32, f32),
}

impl Mixer {
    pub fn new(sample_rate: u32) -> Mixer {
        let sr = sample_rate as f32;
        let vol = Volumes::default();
        Mixer { sr, voices: Vec::new(), engine: Engine::default(), music: Music::new(sr), rng: Rng::new(7), vol, gains: (vol.master * vol.effects, vol.master * vol.music) }
    }

    pub fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play { sfx, gain, pan } => {
                if self.voices.len() >= MAX_VOICES {
                    self.voices.remove(0);
                }
                self.voices.push(Voice::new(sfx, gain.clamp(0.0, 1.0), pan));
            }
            Cmd::Engine(level) => self.engine.target = level.clamp(0.0, 1.0),
            Cmd::Volumes(v) => self.vol = v,
        }
    }

    /// Fill interleaved stereo frames (`out.len()` is a multiple of 2).
    pub fn render(&mut self, out: &mut [f32]) {
        let sr = self.sr;
        let target = if self.vol.muted { (0.0, 0.0) } else { (self.vol.master * self.vol.effects, self.vol.master * self.vol.music) };
        let smooth = 1.0 - (-1.0 / (0.05 * sr)).exp();
        for frame in out.chunks_exact_mut(2) {
            self.gains.0 += (target.0 - self.gains.0) * smooth;
            self.gains.1 += (target.1 - self.gains.1) * smooth;
            let white = self.rng.f32() * 2.0 - 1.0;
            let (mut l, mut r) = (0.0, 0.0);
            for v in &mut self.voices {
                let s = v.sample(white, sr);
                l += s * v.pan.0;
                r += s * v.pan.1;
            }
            let e = self.engine.sample(white, sr) * 0.35;
            l += e;
            r += e;
            let (ml, mr) = self.music.sample(&self.rng, sr);
            // Soft clip: loud pile-ups saturate gently instead of distorting harshly.
            frame[0] = (l * self.gains.0 + ml * self.gains.1).tanh();
            frame[1] = (r * self.gains.0 + mr * self.gains.1).tanh();
        }
        self.voices.retain(|v| !v.done());
    }
}

// ── Output device ──────────────────────────────────────────────────────────

pub struct Audio {
    tx: Sender<Cmd>,
    /// Dropping the stream stops the sound, so it lives as long as this.
    _stream: cpal::Stream,
}

impl Audio {
    /// Open the default output device. Returns None (and the game stays silent) if
    /// there is no device or it can't be opened.
    pub fn start() -> Option<Audio> {
        match Self::try_start() {
            Ok(a) => Some(a),
            Err(e) => {
                crate::vkctx::log(&format!("No sound: {e:#}"));
                None
            }
        }
    }

    fn try_start() -> anyhow::Result<Audio> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        use cpal::SampleFormat;
        let device = cpal::default_host().default_output_device().ok_or_else(|| anyhow::anyhow!("no output device"))?;
        let supported = device.default_output_config()?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let (tx, rx) = channel();
        let mixer = Mixer::new(config.sample_rate);
        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, &config, mixer, rx)?,
            SampleFormat::I16 => build::<i16>(&device, &config, mixer, rx)?,
            SampleFormat::U16 => build::<u16>(&device, &config, mixer, rx)?,
            SampleFormat::I32 => build::<i32>(&device, &config, mixer, rx)?,
            other => anyhow::bail!("unsupported sample format {other}"),
        };
        stream.play()?;
        Ok(Audio { tx, _stream: stream })
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, mut mixer: Mixer, rx: Receiver<Cmd>) -> anyhow::Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait;
    let channels = config.channels as usize;
    let mut stereo: Vec<f32> = Vec::new();
    let stream = device.build_output_stream(
        config.clone(),
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            while let Ok(cmd) = rx.try_recv() {
                mixer.handle(cmd);
            }
            let frames = data.len() / channels.max(1);
            stereo.resize(frames * 2, 0.0);
            mixer.render(&mut stereo);
            for (frame, lr) in data.chunks_exact_mut(channels.max(1)).zip(stereo.chunks_exact(2)) {
                match frame.len() {
                    1 => frame[0] = T::from_sample((lr[0] + lr[1]) * 0.5),
                    _ => {
                        frame[0] = T::from_sample(lr[0]);
                        frame[1] = T::from_sample(lr[1]);
                        for s in &mut frame[2..] {
                            *s = T::from_sample(0.0f32);
                        }
                    }
                }
            }
        },
        |e| crate::vkctx::log(&format!("Sound error: {e}")),
        None,
    )?;
    Ok(stream)
}

// ── Game -> sound ──────────────────────────────────────────────────────────

use crate::camera::CameraFrame;
use crate::game::Game;
use crate::universe::Sound;

/// Turns what happens in the game into mixer commands, once a frame.
#[derive(Default)]
pub struct SoundBridge {
    audio: Option<Audio>,
    volumes: Option<Volumes>,
    engine: f32,
    step_timer: f64,
    /// Real time each effect last played, to thin out rapid repeats.
    last_played: Vec<(Sfx, f64)>,
}

/// Same effect again within this many seconds is skipped (unless it's louder).
const REPEAT_GAP: f64 = 0.06;

fn sfx_for(s: Sound) -> (Sfx, f32) {
    match s {
        Sound::Thud => (Sfx::Thud, 0.7),
        Sound::Smash => (Sfx::Smash, 1.0),
        Sound::StarMerge => (Sfx::Boom, 1.0),
        Sound::BlackHoleMerge => (Sfx::BlackHoleMerge, 1.0),
        Sound::Supernova => (Sfx::Supernova, 1.0),
        Sound::Crash => (Sfx::Crash, 1.0),
        Sound::LiftOff => (Sfx::LiftOff, 0.8),
        Sound::Land => (Sfx::Land, 0.8),
        Sound::Pop => (Sfx::Pop, 0.45),
        Sound::Whoosh => (Sfx::Whoosh, 0.6),
    }
}

/// How loud an event `size` big sounds `distance` away (0..1), and the pan.
pub fn loudness(size: f32, distance: f64) -> f32 {
    let reach = 40.0 + size as f64 * 60.0;
    (reach / (reach + distance)) as f32
}

impl SoundBridge {
    pub fn new(audio: Option<Audio>) -> SoundBridge {
        SoundBridge { audio, ..Default::default() }
    }

    fn play(&mut self, sfx: Sfx, gain: f32, pan: f32, now: f64) {
        let Some(audio) = &self.audio else { return };
        if gain < 0.04 {
            return;
        }
        if let Some((_, t)) = self.last_played.iter_mut().find(|(s, _)| *s == sfx) {
            if now - *t < REPEAT_GAP {
                return;
            }
            *t = now;
        } else {
            self.last_played.push((sfx, now));
        }
        audio.send(Cmd::Play { sfx, gain, pan });
    }

    pub fn update(&mut self, game: &mut Game, cam: &CameraFrame, dt: f64, ui_clicked: bool) {
        let sounds = std::mem::take(&mut game.universe.sounds);
        if self.audio.is_none() {
            return;
        }
        let s = &game.settings;
        let v = Volumes { master: s.master_volume, effects: s.effects_volume, music: s.music_volume, muted: s.muted };
        if self.volumes != Some(v) {
            self.volumes = Some(v);
            if let Some(a) = &self.audio {
                a.send(Cmd::Volumes(v));
            }
        }
        let engine = game.engine_level();
        if (engine - self.engine).abs() > 0.01 || (engine == 0.0) != (self.engine == 0.0) {
            self.engine = engine;
            if let Some(a) = &self.audio {
                a.send(Cmd::Engine(engine));
            }
        }
        let now = game.real_time;
        for e in sounds {
            let (sfx, base) = sfx_for(e.sound);
            let (gain, pan) = match e.pos {
                None => (base * e.size.clamp(0.2, 1.0), 0.0),
                Some(p) => {
                    let to = p - cam.eye;
                    let pan = to.try_normalize().map(|d| d.as_vec3().dot(cam.right) * 0.8).unwrap_or(0.0);
                    (base * loudness(e.size, to.length()), pan)
                }
            };
            self.play(sfx, gain, pan, now);
        }
        match game.footstep_interval() {
            Some(gap) => {
                self.step_timer += dt;
                if self.step_timer >= gap {
                    self.step_timer = 0.0;
                    self.play(Sfx::Step, 0.5, 0.0, now);
                }
            }
            // The first step lands as soon as you start walking.
            None => self.step_timer = 1.0,
        }
        if ui_clicked {
            self.play(Sfx::Click, 0.25, 0.0, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn render(m: &mut Mixer, seconds: f32) -> Vec<f32> {
        let mut out = vec![0.0; (seconds * SR as f32) as usize * 2];
        m.render(&mut out);
        out
    }

    fn peak(s: &[f32]) -> f32 {
        s.iter().fold(0.0f32, |a, x| a.max(x.abs()))
    }

    #[test]
    fn every_effect_makes_a_sound_and_then_stops() {
        for sfx in [Sfx::Thud, Sfx::Smash, Sfx::Boom, Sfx::BlackHoleMerge, Sfx::Supernova, Sfx::Crash, Sfx::LiftOff, Sfx::Land, Sfx::Pop, Sfx::Whoosh, Sfx::Step, Sfx::Click] {
            let mut m = Mixer::new(SR);
            m.handle(Cmd::Volumes(Volumes { master: 1.0, effects: 1.0, music: 0.0, muted: false }));
            m.handle(Cmd::Play { sfx, gain: 1.0, pan: 0.0 });
            let s = render(&mut m, recipe(sfx).dur + 0.1);
            assert!(s.iter().all(|x| x.is_finite() && x.abs() <= 1.0), "{sfx:?}");
            assert!(peak(&s) > 0.05, "{sfx:?} is silent: peak {}", peak(&s));
            assert!(m.voices.is_empty(), "{sfx:?} never finished");
        }
    }

    #[test]
    fn panning_puts_the_sound_on_one_side() {
        let mut m = Mixer::new(SR);
        m.handle(Cmd::Volumes(Volumes { master: 1.0, effects: 1.0, music: 0.0, muted: false }));
        m.handle(Cmd::Play { sfx: Sfx::Thud, gain: 1.0, pan: 1.0 });
        let s = render(&mut m, 0.3);
        let left = peak(&s.iter().step_by(2).copied().collect::<Vec<_>>());
        let right = peak(&s.iter().skip(1).step_by(2).copied().collect::<Vec<_>>());
        assert!(right > 0.1 && left < right * 0.01, "left {left} right {right}");
    }

    #[test]
    fn the_engine_follows_the_throttle() {
        let mut m = Mixer::new(SR);
        m.handle(Cmd::Volumes(Volumes { master: 1.0, effects: 1.0, music: 0.0, muted: false }));
        let idle = peak(&render(&mut m, 0.3));
        m.handle(Cmd::Engine(1.0));
        let full = peak(&render(&mut m, 0.5)[SR as usize / 2..]);
        m.handle(Cmd::Engine(0.0));
        render(&mut m, 0.5);
        let off = peak(&render(&mut m, 0.2));
        // A steady drone under everything else, not a wall of noise.
        assert!(idle < 1e-3 && full > 0.05 && full < 0.6 && off < 1e-3, "idle {idle} full {full} off {off}");
    }

    #[test]
    fn music_plays_and_mute_silences_everything() {
        let mut m = Mixer::new(SR);
        let s = render(&mut m, 4.0);
        let late = peak(&s[s.len() / 2..]);
        assert!(late > 0.01 && late < 0.9, "music peak {late}");
        assert!(s.iter().all(|x| x.is_finite()));
        m.handle(Cmd::Volumes(Volumes { muted: true, ..Volumes::default() }));
        m.handle(Cmd::Play { sfx: Sfx::Supernova, gain: 1.0, pan: 0.0 });
        render(&mut m, 0.5);
        assert!(peak(&render(&mut m, 0.5)) < 1e-4);
    }

    /// Every effect in turn over the music, then the engine revving, written as a WAV to
    /// listen to while tuning: `AUP_DEMO_WAV=demo.wav cargo test --release demo_wav -- --ignored`
    #[test]
    #[ignore]
    fn demo_wav() {
        let path = std::env::var("AUP_DEMO_WAV").unwrap_or_else(|_| "aup-demo.wav".into());
        let mut m = Mixer::new(SR);
        let mut all = render(&mut m, 2.0);
        for (i, sfx) in [Sfx::Click, Sfx::Pop, Sfx::Whoosh, Sfx::Step, Sfx::Step, Sfx::Land, Sfx::Thud, Sfx::LiftOff, Sfx::Crash, Sfx::Smash, Sfx::Boom, Sfx::BlackHoleMerge, Sfx::Supernova]
            .into_iter()
            .enumerate()
        {
            m.handle(Cmd::Play { sfx, gain: 1.0, pan: if i % 2 == 0 { -0.4 } else { 0.4 } });
            all.extend(render(&mut m, recipe(sfx).dur.max(0.4) + 0.3));
        }
        for level in [0.3, 0.7, 1.0, 0.0] {
            m.handle(Cmd::Engine(level));
            all.extend(render(&mut m, 1.2));
        }
        all.extend(render(&mut m, 4.0));
        let data: Vec<u8> = all.iter().flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        for v in [16u32, 1 | (2 << 16), SR, SR * 4, 4 | (16 << 16)] {
            wav.extend_from_slice(&v.to_le_bytes());
        }
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        std::fs::write(&path, wav).unwrap();
        eprintln!("wrote {path} ({:.0} s)", all.len() as f32 / 2.0 / SR as f32);
    }

    #[test]
    fn far_away_is_quieter_and_big_carries_further() {
        assert!(loudness(2.0, 50.0) > loudness(2.0, 1000.0));
        assert!(loudness(200.0, 1000.0) > loudness(2.0, 1000.0));
        assert!(loudness(200.0, 1000.0) > 0.9, "a supernova is heard across the system");
        assert!(loudness(0.2, 2000.0) < 0.04, "a pebble far away is not heard at all");
    }

    #[test]
    fn the_game_queues_sounds_for_what_happens() {
        use crate::presets::Preset;
        let mut g = Game::new();
        g.load_preset(Preset::Supernova);
        let star = g.universe.bodies.iter().find(|b| b.supernova_timer.is_some()).map(|b| b.id).unwrap();
        g.universe.get_mut(star).unwrap().supernova_timer = Some(0.01);
        let cam = g.camera.frame(1600.0, 1000.0);
        for _ in 0..30 {
            g.update(1.0 / 60.0, &cam, false);
        }
        assert!(g.universe.sounds.iter().any(|s| s.sound == Sound::Supernova), "{:?}", g.universe.sounds);
        // Without a sound device the bridge still drains the queue.
        let mut bridge = SoundBridge::new(None);
        bridge.update(&mut g, &cam, 1.0 / 60.0, false);
        assert!(g.universe.sounds.is_empty());
    }

    #[test]
    fn a_pile_of_explosions_stays_in_range() {
        let mut m = Mixer::new(SR);
        for _ in 0..100 {
            m.handle(Cmd::Play { sfx: Sfx::Supernova, gain: 1.0, pan: 0.0 });
        }
        assert_eq!(m.voices.len(), MAX_VOICES);
        let s = render(&mut m, 1.0);
        assert!(s.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
    }
}
