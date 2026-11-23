//! Frame orchestration, backdrop hashing, GPU profiler and render-pass dispatch.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rune::compose::Frame;
use rune::render::{EffectPhase, EffectSite, Instance};
use rune_fx::{FxFrame, FxOptions, FxRenderer};

use super::gpu::{Binds, Gpu, Pipe};
use super::slots::{APPEAR_S, BarMem, FrameCtx};
use super::stage::find_cover_rect;
use super::{
    BARS, BUTTERFLY, FLOW_EASE_S, FLOW_S, GLASS_READY, GPU_UP, Gfx, LAST_DRAW_CLOCK, MORPH, OFF_FPS, READY, SHAPES_READY, STAT_PYR_BYTES, STAT_PYR_COUNT, STAT_UBUFS, TICK_ARMED,
    WANTS_TICK, off_mask,
};
use crate::app::State;
use crate::store::Effects;
use crate::theme;

/// Dev tool (`GLINT_FXOFF=fps`): GPU frame time read back via timestamp queries.
pub(super) struct Prof {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    pending: Arc<AtomicBool>,
    state: u8,
    sum_ms: f64,
    n: u32,
    max_ms: f64,
}

fn art_id(key: Option<&str>) -> usize {
    key.map_or(0, |k| k.bytes().fold(0xcbf2_9ce4_8422_2325u64, |a, b| (a ^ b as u64).wrapping_mul(0x100_0000_01b3)) as usize)
}

/// What the backdrop behind `SNAP` looks like (instances `0..limit`).
fn look_of(f: &Frame<'_, State>, limit: usize) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |w: u64| {
        h = (h ^ w).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        h ^= h >> 29;
    };
    let st = &f.rt.state;
    eat(u64::from(f.size.0) << 32 | u64::from(f.size.1));
    eat(st.settings.effects as u64 | u64::from(st.settings.reduce_motion) << 8 | u64::from(theme::light()) << 9);
    for (n, instance) in f.rt.instances.iter().take(limit).enumerate() {
        if instance.rect[2] <= 0.0 || instance.rect[3] <= 0.0 {
            continue;
        }
        let mark = f.rt.marks.get(n).copied().unwrap_or(0);
        if matches!(mark & 0xff, BARS | MORPH | BUTTERFLY) {
            continue;
        }
        for word in bytemuck::cast_slice::<Instance, u32>(std::slice::from_ref(instance)) {
            eat(u64::from(*word));
        }
        eat(u64::from(mark));
    }
    h
}

impl Gfx {
    pub(super) fn draw(&mut self, f: &mut Frame<'_, State>) {
        LAST_DRAW_CLOCK.store(f.rt.clock.to_bits(), Ordering::Relaxed);
        let (device, queue, size) = (f.device, f.queue, f.size);
        let pass_size = [size.0, size.1];
        let st = &f.rt.state;
        let quality = st.settings.effects;
        let reduced = st.settings.reduce_motion;
        let accent = theme::accent_srgb(st.settings.accent, self.smooth.accent);
        let dt = f.dt.clamp(0.0, 0.1) as f32;
        self.smooth.update(dt, st);
        let t = if reduced { 7.3 } else { f.rt.t as f32 };
        let srgb = f.srgb_blend();
        let (canvas, ink) = (theme::canvas(), theme::ink());
        let playing_level = self.smooth.level > 0.002;
        if !reduced && quality != Effects::Off {
            self.field_t += dt * (0.12 + self.smooth.level * 1.4);
        }
        if playing_level || !self.flowing {
            self.idle = 0.0;
        } else {
            self.idle += dt;
        }
        let flow = ((FLOW_S + FLOW_EASE_S - self.idle) / FLOW_EASE_S).clamp(0.0, 1.0);
        let flow = flow * flow * (3.0 - 2.0 * flow);
        if !reduced && quality != Effects::Off {
            let want = if st.player.playing { self.smooth.norm[3] } else { 0.0 };
            self.shape_music += (want - self.shape_music) * (1.0 - (-dt * 3.0).exp());
            self.shape_t += dt * flow * (1.0 + 0.5 * self.shape_music);
        }
        let stage_fx = st.settings.stage.effects;
        let heard = st.player.playing && self.smooth.level > 0.002;
        let song_live = heard && !reduced && quality == Effects::Full && stage_fx > 0;
        if quality != Effects::Off && stage_fx > 0 {
            self.song.hear(dt, &st.tone, &st.spectrum, &st.drive, self.smooth.norm[3], heard);
        }
        if song_live {
            let pace = if stage_fx == 1 { 0.45 } else { 1.0 };
            self.song.advance(dt * pace * (self.smooth.tempo / 120.0).clamp(0.75, 1.5));
        }
        if !reduced && quality == Effects::Full && stage_fx > 0 {
            let pace = if stage_fx == 1 { 0.45 } else { 1.0 };
            self.song.ground_t += dt * flow * pace;
        }
        let bars_want = if st.player.playing && !reduced && quality != Effects::Off { 1.0 } else { 0.0 };
        let rate = if bars_want > self.bars_live { 9.0 } else { 6.0 };
        self.bars_live += (bars_want - self.bars_live) * (1.0 - (-dt * rate).exp());
        if (bars_want - self.bars_live).abs() < 0.002 {
            self.bars_live = bars_want;
        }
        self.bars_t += dt * self.bars_live;
        let bars_were_up = self.bars.iter().any(|b| b.seed >= 0.0 && b.age >= APPEAR_S);
        if self.morph_on && !reduced {
            self.morph_t += dt;
        }
        self.morph_on = false;

        let mut gpu = match self.gpu.take() {
            Some(g) if g.key == f.device_key() => g,
            // A new device (the window was hidden and gave its GPU back): the like burst's
            // renderer was made on the old one too.
            _ => {
                self.fx = None;
                self.prof = None;
                Gpu::new(device, f.device_key())
            }
        };
        gpu.ensure(device, f.format);
        let first_gpu = !GPU_UP.load(Ordering::Relaxed) && !GPU_UP.swap(true, Ordering::Relaxed);
        let glass_up = !GLASS_READY.load(Ordering::Relaxed) && gpu.glass_ready(f.format) && !GLASS_READY.swap(true, Ordering::Relaxed);
        let shapes_up = !SHAPES_READY.load(Ordering::Relaxed) && gpu.shapes_ready(f.format) && !SHAPES_READY.swap(true, Ordering::Relaxed);
        let core_up = !READY.load(Ordering::Relaxed) && gpu.core_ready(f.format) && !READY.swap(true, Ordering::Relaxed);
        let building = !gpu.ready();
        let came_up = first_gpu || glass_up || shapes_up || core_up;
        gpu.begin();
        if quality != Effects::Off
            && let Some(th) = &st.cover_thumb
            && gpu.set_art(device, queue, art_id(st.palette_key.as_deref()), th)
        {
            self.art_change = 0.0;
        }
        self.art_change = if reduced { 99.0 } else { self.art_change + dt };
        let art_fading = gpu.step_art(queue, dt, reduced);
        let cover_mix = (self.art_change / 1.1).clamp(0.0, 1.0);
        let fps_on = off_mask() & OFF_FPS != 0;
        if fps_on && device.features().contains(wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
            self.prof_begin(device, queue, f.encoder);
        }

        let cover_rect = if st.np_open || st.np.t > 0.001 { find_cover_rect(&f.rt.instances, &f.rt.marks) } else { None };
        let ctx =
            FrameCtx { device, queue, st, pass_size, srgb, dt, t, quality, reduced, playing_level, song_live, flow, cover_mix, canvas, ink, accent, bars_were_up, cover_rect };
        let summary = self.prepare_slots(&mut gpu, &f.rt.instances, &f.rt.marks, &ctx);

        let snap_limit = self.snaps.last().copied().unwrap_or(0) as usize;
        let look = look_of(f, snap_limit);
        let fast_dynamic = !reduced
            && (summary.moving || self.smooth.level > 0.002 || self.song.loud > 0.002)
            && self
                .jobs
                .iter()
                .take_while(|(n, _)| (*n as usize) < snap_limit)
                .any(|(_, job)| matches!(job.pipe, Pipe::Bars | Pipe::Wave | Pipe::Mood | Pipe::Ribbon | Pipe::Butterfly | Pipe::Stage));
        let field_dynamic =
            !reduced && quality != Effects::Off && playing_level && self.jobs.iter().take_while(|(n, _)| (*n as usize) < snap_limit).any(|(_, job)| job.pipe == Pipe::Field);
        let pyramid_key = if fast_dynamic {
            look ^ ((f.rt.clock * 12.0) as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        } else if field_dynamic {
            look ^ ((f.rt.clock * 2.0) as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        } else {
            look
        };
        self.flowing = summary.flows;
        for m in self.bars.iter_mut().skip(summary.bars_n) {
            m.gone += dt;
            if m.gone > 0.25 {
                *m = BarMem::NEW;
            }
        }
        if summary.shapes_present {
            self.shapes_gone = 0.0;
        } else {
            self.shapes_gone += dt;
            if self.shapes_gone > 0.25 {
                self.shapes.clear();
            }
        }
        let audio_live = quality != Effects::Off && !reduced && summary.audio_reactive && (self.smooth.level > 0.002 || self.smooth.beat > 0.01 || self.smooth.stirring());
        let want_tick = summary.moving || audio_live || art_fading;
        let armed = TICK_ARMED.swap(false, Ordering::Relaxed);
        WANTS_TICK.store(want_tick, Ordering::Relaxed);
        let need_kick = want_tick && !armed;
        let ui_moving = st.ui_animating || st.menu_moving() || st.scroll_restore.is_some();
        self.animating = building || came_up || ui_moving || need_kick;
        self.drew_own = !self.jobs.is_empty() || !self.layers.is_empty();
        if fps_on {
            self.log_fps(f.rt.clock);
        }

        if !self.layers.is_empty() {
            let fx = self.fx.get_or_insert_with(|| FxRenderer::new(device, f.format, FxOptions { dither: true }));
            fx.set_srgb_blend(srgb);
            fx.prepare_with_slots(device, queue, &FxFrame { viewport: [size.0, size.1], scale: 1.0, layers: &self.layers, backdrop: None, sources: &[] }, &self.placements);
        }

        let clear = f.rt.clear_color();
        let timing = self.prof.as_ref().is_some_and(|p| p.state == 0);
        let mut snap: Option<wgpu::TextureView> = None;
        gpu.ensure_uniform_capacity(device, gpu.used() + 16);
        if let Some((buf, bytes)) = gpu.uniform_upload() {
            f.write_buffer(buf, 0, bytes);
        }
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, size);
        let jobs = &self.jobs;
        let snaps = &self.snaps;
        let at = &self.at;
        let fx_opt = &mut self.fx;
        f.ui.draw_with_backdrop(f.encoder, f.target, clear, &mut |phase, s: &EffectSite<'_>| {
            if snaps.binary_search(&s.index).is_ok() {
                if !s.in_layer
                    && let EffectPhase::Backdrop(enc) = phase
                    && let Some(bd) = s.backdrop
                {
                    let next = snaps.iter().copied().find(|&n| n > s.index).unwrap_or(u32::MAX);
                    let start = jobs.partition_point(|(n, _)| *n <= s.index);
                    for (n, other) in &jobs[start..] {
                        if *n >= next {
                            break;
                        }
                        if !other.shared
                            && let Some(r) = &other.region
                        {
                            gpu.pyramid(device, enc, bd, other.u, r, other.tag, pyramid_key);
                        }
                    }
                    snap = Some(bd.clone());
                }
                return;
            }
            if let Ok(pos) = jobs.binary_search_by_key(&s.index, |(n, _)| *n) {
                let job = &jobs[pos].1;
                if s.in_layer {
                    if job.pipe == Pipe::Veil || (job.pipe == Pipe::Glass && job.region.is_some()) {
                        return;
                    }
                    if let EffectPhase::Draw(pass) = phase
                        && gpu.ensure_layer_pipe(device, s.format, job.pipe)
                    {
                        let u = gpu.upload_in_layer(device, queue, job.u, s.origin, s.size);
                        let b = Binds { bd: None, pyr: None, tag: job.tag };
                        gpu.draw(device, pass, s.format, job.pipe, u, b);
                    }
                    return;
                }
                if let EffectPhase::Draw(pass) = phase {
                    let reads_backdrop = matches!(job.pipe, Pipe::Glass | Pipe::Veil);
                    if job.pipe == Pipe::Veil && snap.is_none() {
                        return;
                    }
                    let b = Binds { bd: if reads_backdrop { snap.as_ref() } else { None }, pyr: job.region.map(|r| r.dims), tag: job.tag };
                    gpu.draw(device, pass, s.format, job.pipe, job.u, b);
                }
                return;
            }
            let Ok(k) = at.binary_search(&s.index) else { return };
            let Some(fx) = fx_opt.as_mut() else { return };
            match phase {
                EffectPhase::Backdrop(enc) => {
                    if let Some(bd) = s.backdrop {
                        fx.blur_backdrop(device, queue, enc, bd, k);
                    }
                }
                EffectPhase::Draw(pass) => {
                    if s.in_layer && s.format != f.format {
                        fx.add_format(device, s.format);
                    }
                    fx.draw_layer_in(pass, s.format, s.origin, k);
                }
            }
        });
        gpu.finish(device, queue, pass_size);
        let (pyrs, pyr_bytes, ubufs) = gpu.stats();
        STAT_PYR_COUNT.store(pyrs, Ordering::Relaxed);
        STAT_PYR_BYTES.store(pyr_bytes, Ordering::Relaxed);
        STAT_UBUFS.store(ubufs, Ordering::Relaxed);
        self.gpu = Some(gpu);
        if let (true, Some(p)) = (timing, self.prof.as_mut()) {
            f.encoder.write_timestamp(&p.set, 1);
            f.encoder.resolve_query_set(&p.set, 0..2, &p.resolve, 0);
            f.encoder.copy_buffer_to_buffer(&p.resolve, 0, &p.read, 0, 16);
            p.state = 1;
        }
    }

    fn prof_begin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder) {
        let p = self.prof.get_or_insert_with(|| Prof {
            set: device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("frame time"), ty: wgpu::QueryType::Timestamp, count: 2 }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame time"),
                size: 16,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            read: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame time"),
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            pending: Default::default(),
            state: 0,
            sum_ms: 0.0,
            n: 0,
            max_ms: 0.0,
        });
        if p.state == 1 {
            let flag = p.pending.clone();
            p.read.slice(..).map_async(wgpu::MapMode::Read, move |_| flag.store(true, Ordering::Release));
            p.state = 2;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        if p.state == 2 && p.pending.load(Ordering::Acquire) {
            let v: [u64; 2] = match p.read.slice(..).get_mapped_range() {
                Ok(data) => match data.split_first_chunk::<8>().and_then(|(a, rest)| rest.split_first_chunk::<8>().map(|(b, _)| (*a, *b))) {
                    Some((a, b)) => [u64::from_le_bytes(a), u64::from_le_bytes(b)],
                    None => [0, 0],
                },
                Err(_) => [0, 0],
            };
            p.read.unmap();
            p.state = 0;
            p.pending.store(false, Ordering::Release);
            let ms = v[1].saturating_sub(v[0]) as f64 * f64::from(queue.get_timestamp_period()) / 1.0e6;
            p.sum_ms += ms;
            p.n += 1;
            p.max_ms = p.max_ms.max(ms);
        }
        if p.state == 0 {
            encoder.write_timestamp(&p.set, 0);
        }
    }

    fn log_fps(&mut self, clock: f64) {
        self.frames += 1;
        if clock - self.fps_at >= 1.0 {
            let gpu_ms = self.prof.as_ref().map_or(String::new(), |p| format!(" gpu {:.2} ms avg, {:.2} max over {} frames", p.sum_ms / f64::from(p.n.max(1)), p.max_ms, p.n));
            let n = f64::from(self.frames.max(1));
            eprintln!("fps {:.1} (drawn {}) runtime {:.2} ms, ours {:.2} ms per frame{gpu_ms}", n / (clock - self.fps_at), self.frames, self.upd_ms / n, self.cpu_ms / n);
            let h = self.song.harmony();
            eprintln!("stage: loud {:.2} harmony {:.0} deg x{:.2}", self.song.loud, h[1].atan2(h[0]).to_degrees(), h[2]);
            if let Some(p) = self.prof.as_mut() {
                (p.sum_ms, p.n, p.max_ms) = (0.0, 0, 0.0);
            }
            (self.frames, self.upd_ms, self.cpu_ms, self.fps_at) = (0, 0.0, 0.0, clock);
        }
    }
}
