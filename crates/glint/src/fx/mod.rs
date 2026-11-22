//! The compositor. Every `El::effect` slot in the view becomes one of Glint's own WGSL
//! effects (the per-effect shaders under `shaders/`: the dithered page, liquid glass, the
//! progressive blur at the page's edges, the seek bars, the playing marker, the garden on Home,
//! the stage behind the open player) or a `rune-fx` layer (the like burst).

mod audio;
mod gpu;
mod render;
mod slots;
mod stage;

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use rune::compose::{Compositor, Frame};
use rune_fx::{FxLayer, FxRenderer, FxSlot};

use crate::app::State;
use audio::{Smooth, Song};
use gpu::Gpu;
use render::Prof;
use slots::{BarMem, Job};

pub use slots::{Glass, glass};

// Marks of the slots Glint paints itself.
pub const FIELD: u32 = 16;
pub const GLASS: u32 = 18;
pub const VEIL: u32 = 19;
pub const SNAP: u32 = 21;
pub const MORPH: u32 = 23;
pub const BARS: u32 = 24;
/// How many values a track's waveform has (`State::waveforms`).
pub const WAVE_N: usize = 256;
/// Invisible marker placed over the playing cover so `STAGE` knows where the cover stands.
pub const COVER_MARK: u32 = 27;
/// A shape of the garden on Home (`fs_wave`, `fs_mood`, `fs_ribbon`).
pub const SHAPE: u32 = 28;
/// Behind the open player (`fs_stage`).
pub const STAGE: u32 = 29;
/// One of the butterflies on Home (`fs_butterfly`).
pub const BUTTERFLY: u32 = 30;
// rune-fx layers.
pub const SPARKLES: u32 = 6;

/// Bitmask parsed once from `GLINT_FXOFF=glass,veil,snap,field,morph,bars,shapes,stage,fps`.
const OFF_GLASS: u16 = 1 << 0;
const OFF_VEIL: u16 = 1 << 1;
const OFF_SNAP: u16 = 1 << 2;
const OFF_FIELD: u16 = 1 << 3;
const OFF_MORPH: u16 = 1 << 4;
const OFF_BARS: u16 = 1 << 5;
const OFF_SHAPES: u16 = 1 << 6;
const OFF_STAGE: u16 = 1 << 7;
const OFF_FPS: u16 = 1 << 8;

fn off_mask() -> u16 {
    static MASK: OnceLock<u16> = OnceLock::new();
    *MASK.get_or_init(|| {
        let mut m = 0u16;
        if let Ok(val) = std::env::var("GLINT_FXOFF") {
            for part in val.split(',') {
                match part.trim() {
                    "glass" => m |= OFF_GLASS,
                    "veil" => m |= OFF_VEIL,
                    "snap" => m |= OFF_SNAP,
                    "field" => m |= OFF_FIELD,
                    "morph" => m |= OFF_MORPH,
                    "bars" => m |= OFF_BARS,
                    "shapes" => m |= OFF_SHAPES,
                    "stage" => m |= OFF_STAGE,
                    "fps" => m |= OFF_FPS,
                    _ => {}
                }
            }
        }
        m
    })
}

fn off_kind(kind: u32) -> bool {
    let m = off_mask();
    match kind {
        GLASS => m & OFF_GLASS != 0,
        VEIL => m & OFF_VEIL != 0,
        SNAP => m & OFF_SNAP != 0,
        FIELD => m & OFF_FIELD != 0,
        MORPH => m & OFF_MORPH != 0,
        BARS => m & OFF_BARS != 0,
        COVER_MARK => true,
        SHAPE | BUTTERFLY => m & OFF_SHAPES != 0,
        STAGE => m & OFF_STAGE != 0,
        _ => false,
    }
}

fn hex3(c: [f32; 3]) -> u32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    (q(c[0]) << 16) | (q(c[1]) << 8) | q(c[2])
}

/// Readiness flags for staged shader bring-up.
pub static READY: AtomicBool = AtomicBool::new(false);
pub static GLASS_READY: AtomicBool = AtomicBool::new(false);
pub static SHAPES_READY: AtomicBool = AtomicBool::new(false);
pub static GPU_UP: AtomicBool = AtomicBool::new(false);

/// Where the pointer is over My Wave's liquid (in liquid units).
pub(super) static WAVE_POINTER: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

pub fn set_wave_pointer(x: f32, y: f32) {
    WAVE_POINTER[0].store(x.to_bits(), Ordering::Relaxed);
    WAVE_POINTER[1].store(y.to_bits(), Ordering::Relaxed);
}

pub(super) static WANTS_TICK: AtomicBool = AtomicBool::new(false);
pub(super) static TICK_ARMED: AtomicBool = AtomicBool::new(false);
pub(super) static EFFECTS_OFF: AtomicBool = AtomicBool::new(false);
pub(super) static LAST_DRAW_CLOCK: AtomicU64 = AtomicU64::new(0);
pub(super) static STAT_PYR_COUNT: AtomicU32 = AtomicU32::new(0);
pub(super) static STAT_PYR_BYTES: AtomicU64 = AtomicU64::new(0);
pub(super) static STAT_UBUFS: AtomicU32 = AtomicU32::new(0);

/// Latest compositor GPU resource counters: `(pyramid_count, pyramid_bytes, uniform_buffer_count)`.
pub fn gpu_stats() -> (usize, usize, usize) {
    (STAT_PYR_COUNT.load(Ordering::Relaxed) as usize, STAT_PYR_BYTES.load(Ordering::Relaxed) as usize, STAT_UBUFS.load(Ordering::Relaxed) as usize)
}

pub fn wants_tick() -> bool {
    WANTS_TICK.load(Ordering::Relaxed)
}

pub fn arm_tick() {
    TICK_ARMED.store(true, Ordering::Relaxed);
}

pub fn disarm_tick() {
    TICK_ARMED.store(false, Ordering::Relaxed);
}

pub fn last_draw_clock() -> f64 {
    f64::from_bits(LAST_DRAW_CLOCK.load(Ordering::Relaxed))
}

pub fn set_effects_off(off: bool) {
    EFFECTS_OFF.store(off, Ordering::Relaxed);
}

pub fn ready() -> bool {
    READY.load(Ordering::Relaxed)
}

pub fn glass_ready() -> bool {
    GLASS_READY.load(Ordering::Relaxed)
}

thread_local! {
    static RESIZING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn set_resizing(active: bool) {
    RESIZING.set(active);
}

/// While the window edge moves or effects are turned off, draw plain tinted panes without
/// allocating or copying a full-window backdrop.
pub fn glass_enabled() -> bool {
    glass_ready() && !RESIZING.get() && !EFFECTS_OFF.load(Ordering::Relaxed)
}

pub fn shapes_ready() -> bool {
    off_mask() & OFF_SHAPES == 0 && SHAPES_READY.load(Ordering::Relaxed)
}

pub fn gpu_up() -> bool {
    GPU_UP.load(Ordering::Relaxed)
}

pub fn snap_slot(el: rune::El) -> rune::El {
    if glass_enabled() { el.effect(SNAP).effect_backdrop(true) } else { el }
}

pub fn veil_slot(el: rune::El, data: [f32; 4]) -> rune::El {
    if glass_enabled() { el.effect(VEIL).effect_data(data) } else { el }
}

/// An effect slot that is left out until the compositor can draw it (see [`READY`]).
pub trait Slot: Sized {
    fn fx(self, mark: u32) -> Self;
    fn fx_data(self, data: [f32; 4]) -> Self;
    fn fx_bleed(self, points: f32) -> Self;
}

impl Slot for rune::El {
    fn fx(self, mark: u32) -> Self {
        if ready() { self.effect(mark) } else { self }
    }
    fn fx_data(self, data: [f32; 4]) -> Self {
        if ready() { self.effect_data(data) } else { self }
    }
    fn fx_bleed(self, points: f32) -> Self {
        if ready() { self.effect_bleed(points) } else { self }
    }
}

/// Seconds the page flows on its own before it slows down, and how long the slowing takes.
pub(super) const FLOW_S: f32 = 2.5;
pub(super) const FLOW_EASE_S: f32 = 1.5;

pub struct Gfx {
    fx: Option<FxRenderer>,
    gpu: Option<Gpu>,
    smooth: Smooth,
    animating: bool,
    drew_own: bool,
    field_t: f32,
    idle: f32,
    morph_t: f32,
    marker_run: f32,
    morph_on: bool,
    morph_avg: [f32; 3],
    morph_bars: [f32; 3],
    shape_t: f32,
    shape_music: f32,
    wave_ptr: [f32; 3],
    shapes: HashMap<u32, [f32; 4]>,
    shapes_gone: f32,
    song: Song,
    art_change: f32,
    bars: [BarMem; 3],
    bars_live: f32,
    bars_t: f32,
    wave_id: String,
    wave_now: Vec<f32>,
    wave_from_real: bool,
    wave_mix: f32,
    flowing: bool,
    /// Reusable per-frame scratch buffers (cleared each frame, avoiding heap churn).
    jobs: Vec<(u32, Job)>,
    snaps: Vec<u32>,
    at: Vec<u32>,
    layers: Vec<FxLayer>,
    placements: Vec<FxSlot>,
    frames: u32,
    upd_ms: f64,
    cpu_ms: f64,
    fps_at: f64,
    prof: Option<Prof>,
}

impl Gfx {
    pub fn new() -> Gfx {
        Gfx {
            fx: None,
            gpu: None,
            smooth: Smooth::new(),
            animating: true,
            drew_own: true,
            field_t: 0.0,
            idle: 0.0,
            morph_t: 0.0,
            marker_run: 0.0,
            morph_on: false,
            morph_avg: [0.05; 3],
            morph_bars: [0.0; 3],
            shape_t: 0.0,
            shape_music: 0.0,
            wave_ptr: [0.0; 3],
            shapes: HashMap::new(),
            shapes_gone: 1.0,
            song: Song::new(),
            art_change: 99.0,
            bars: [BarMem::NEW; 3],
            bars_live: 0.0,
            bars_t: 0.0,
            wave_id: String::new(),
            wave_now: Vec::new(),
            wave_from_real: false,
            wave_mix: 0.0,
            flowing: false,
            jobs: Vec::new(),
            snaps: Vec::new(),
            at: Vec::new(),
            layers: Vec::new(),
            placements: Vec::new(),
            frames: 0,
            upd_ms: 0.0,
            cpu_ms: 0.0,
            fps_at: 0.0,
            prof: None,
        }
    }
}

impl Compositor<State> for Gfx {
    fn features(&self) -> wgpu::Features {
        if off_mask() & OFF_FPS != 0 { wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS } else { wgpu::Features::empty() }
    }

    fn animating(&self) -> bool {
        self.animating
    }

    fn draws_own(&self) -> bool {
        self.drew_own
    }

    fn render(&mut self, f: &mut Frame<'_, State>) {
        let began = std::time::Instant::now();
        self.upd_ms += f.update_ms;
        self.draw(f);
        self.cpu_ms += began.elapsed().as_secs_f64() * 1e3;
    }
}
