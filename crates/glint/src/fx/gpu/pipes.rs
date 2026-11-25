//! Shader sources and asynchronous background pipeline compiler.

use rune::render::CLIP_WGSL;

use super::{Gpu, HDR, PYR_FMT};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pipe {
    Field,
    Glass,
    Veil,
    Morph,
    /// The seek line.
    Bars,
    /// Behind the open player (`fs_stage`): reads the playing cover and live music readings.
    Stage,
    /// Stage Home 3D-texture fluid wave (`fs_wave_stage`).
    Wave,
    /// Mood pebbles and pictures (`fs_mood`).
    Mood,
    /// A wave card's ribbons (`fs_ribbon`).
    Ribbon,
    /// A butterfly (`fs_butterfly`).
    Butterfly,
    /// The first pyramid level (bilinear from backdrop), and every level after it (`textureLoad`).
    Down0,
    Down,
}

impl Pipe {
    fn entry(self) -> &'static str {
        match self {
            Pipe::Field => "fs_field",
            Pipe::Glass => "fs_glass",
            Pipe::Veil => "fs_veil",
            Pipe::Morph => "fs_morph",
            Pipe::Bars => "fs_bars",
            Pipe::Wave => "fs_wave_stage",
            Pipe::Mood => "fs_mood",
            Pipe::Ribbon => "fs_ribbon",
            Pipe::Butterfly => "fs_butterfly",
            Pipe::Stage => "fs_stage",
            Pipe::Down0 => "fs_down0",
            Pipe::Down => "fs_down",
        }
    }

    fn is_down(self) -> bool {
        matches!(self, Pipe::Down0 | Pipe::Down)
    }

    fn module_key(self) -> Pipe {
        if self.is_down() { Pipe::Down0 } else { self }
    }

    /// Pipelines that copy instead of blending over the frame.
    fn opaque(self) -> bool {
        matches!(self, Pipe::Down0 | Pipe::Down)
    }
}

/// The format of RUNE's layers, and the slots that can be drawn into one (all but the ones that
/// read the frame behind them).
const LAYER: wgpu::TextureFormat = HDR;
const LAYER_PIPES: [Pipe; 8] = [Pipe::Field, Pipe::Morph, Pipe::Bars, Pipe::Wave, Pipe::Mood, Pipe::Ribbon, Pipe::Butterfly, Pipe::Stage];

fn shader_source(pipe: Pipe) -> String {
    const COMMON: &str = include_str!("../../shaders/common.wgsl");
    const GFX_HEAD: &str = include_str!("../../shaders/gfx_head.wgsl");
    const BLUR: &str = include_str!("../../shaders/blur.wgsl");
    const NOISE: &str = include_str!("../../shaders/noise.wgsl");
    const SHAPE_HELPERS: &str = include_str!("../../shaders/shape_helpers.wgsl");
    if pipe.is_down() {
        return format!("{COMMON}\n{}", include_str!("../../shaders/down.wgsl"));
    }
    if pipe == Pipe::Wave {
        // Standalone 3D-texture wave shader (no `gfx_head.wgsl` 2D bindings).
        return format!("{CLIP_WGSL}\n{COMMON}\n{NOISE}\n{SHAPE_HELPERS}\n{}", include_str!("../../shaders/wave_stage.wgsl"));
    }
    let body = match pipe {
        Pipe::Field => include_str!("../../shaders/field.wgsl"),
        Pipe::Glass => include_str!("../../shaders/glass.wgsl"),
        Pipe::Veil => include_str!("../../shaders/veil.wgsl"),
        Pipe::Morph => include_str!("../../shaders/morph.wgsl"),
        Pipe::Bars => include_str!("../../shaders/bars.wgsl"),
        Pipe::Mood => include_str!("../../shaders/mood.wgsl"),
        Pipe::Ribbon => include_str!("../../shaders/ribbon.wgsl"),
        Pipe::Butterfly => include_str!("../../shaders/butterfly.wgsl"),
        Pipe::Stage => include_str!("../../shaders/stage.wgsl"),
        Pipe::Wave | Pipe::Down0 | Pipe::Down => unreachable!(),
    };
    let mut src = format!("{CLIP_WGSL}\n{COMMON}\n{GFX_HEAD}\n");
    if matches!(pipe, Pipe::Glass | Pipe::Veil) {
        src.push_str(BLUR);
        src.push('\n');
    }
    if matches!(pipe, Pipe::Mood | Pipe::Butterfly | Pipe::Stage) {
        src.push_str(NOISE);
        src.push('\n');
    }
    if matches!(pipe, Pipe::Mood | Pipe::Ribbon | Pipe::Butterfly | Pipe::Stage) {
        src.push_str(SHAPE_HELPERS);
        src.push('\n');
    }
    src.push_str(body);
    src
}

fn build_pipe(device: &wgpu::Device, module: &wgpu::ShaderModule, layout: &wgpu::PipelineLayout, vertex: &str, pipe: Pipe, target: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(pipe.entry()),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some(vertex), compilation_options: Default::default(), buffers: &[] },
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(pipe.entry()),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target,
                blend: if pipe.opaque() { None } else { Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING) },
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl Gpu {
    /// Queues only the specific pipelines `wanted` needs (compiles each shader lazily on first
    /// use instead of compiling all 12 pipelines at startup, cutting startup DXC/driver RAM).
    pub fn ensure_pipes(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat, wanted: &[Pipe]) {
        self.take_warm();
        let mut missing: Vec<(wgpu::TextureFormat, Pipe)> = Vec::new();
        let mut push_one = |target: wgpu::TextureFormat, pipe: Pipe| {
            let key = (target, pipe);
            if !self.pipes.contains_key(&key) && self.queued.insert(key) {
                missing.push(key);
            }
        };
        let mut need_down = false;
        for &p in wanted {
            if matches!(p, Pipe::Glass | Pipe::Veil) {
                need_down = true;
            }
            let target = if p.is_down() { PYR_FMT } else { format };
            push_one(target, p);
        }
        if need_down {
            push_one(PYR_FMT, Pipe::Down0);
            push_one(PYR_FMT, Pipe::Down);
        }
        if missing.is_empty() {
            return;
        }
        // Group `Down0` + `Down` into one job so they share their compiled shader module.
        let mut down_job: Vec<(wgpu::TextureFormat, Pipe)> = Vec::new();
        let mut jobs: Vec<Vec<(wgpu::TextureFormat, Pipe)>> = Vec::with_capacity(missing.len());
        for pair in missing {
            if pair.1.is_down() {
                down_job.push(pair);
            } else {
                jobs.push(vec![pair]);
            }
        }
        if !down_job.is_empty() {
            jobs.push(down_job);
        }
        // Headless snapshots build synchronously on demand so every effect is ready immediately.
        if crate::app::is_headless() {
            for job in jobs {
                let mut cached_module: Option<(Pipe, wgpu::ShaderModule)> = None;
                for (target, pipe) in job {
                    let mod_key = pipe.module_key();
                    let module = match &cached_module {
                        Some((k, m)) if *k == mod_key => m,
                        _ => {
                            let label = if pipe.is_down() { "glint down" } else { pipe.entry() };
                            let m = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(shader_source(pipe).into()) });
                            &cached_module.insert((mod_key, m)).1
                        }
                    };
                    let (layout, vertex) = match pipe {
                        Pipe::Down0 => (&self.down0_layout, "vs_full"),
                        Pipe::Down => (&self.down_layout, "vs_full"),
                        Pipe::Wave => (&self.wave_layout, "vs"),
                        _ => (&self.gfx_layout, "vs"),
                    };
                    let built = build_pipe(device, module, layout, vertex, pipe, target);
                    self.pipes.entry((target, pipe)).or_insert(built);
                }
            }
            return;
        }
        // Build lazily on up to 2 worker threads so startup shaders compile concurrently while
        // keeping peak DXC/FXC driver heap low.
        let max_threads = if jobs.len() >= 2 { 2 } else { 1 };
        let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get).clamp(1, max_threads).min(jobs.len());
        let mut chunks: Vec<Vec<(wgpu::TextureFormat, Pipe)>> = vec![Vec::new(); threads];
        for (i, job) in jobs.drain(..).enumerate() {
            chunks[i % threads].extend(job);
        }
        for chunk in chunks.into_iter().filter(|c| !c.is_empty()) {
            let (tx, rx) = std::sync::mpsc::channel();
            let (device, gfx_layout, wave_layout, down0_layout, down_layout) =
                (device.clone(), self.gfx_layout.clone(), self.wave_layout.clone(), self.down0_layout.clone(), self.down_layout.clone());
            let spawned = std::thread::Builder::new().name("glint-pipelines".into()).spawn(move || {
                let mut cached_module: Option<(Pipe, wgpu::ShaderModule)> = None;
                for (target, pipe) in chunk {
                    let mod_key = pipe.module_key();
                    let module = match &cached_module {
                        Some((k, m)) if *k == mod_key => m,
                        _ => {
                            let label = if pipe.is_down() { "glint down" } else { pipe.entry() };
                            let m = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(shader_source(pipe).into()) });
                            &cached_module.insert((mod_key, m)).1
                        }
                    };
                    let (layout, vertex) = match pipe {
                        Pipe::Down0 => (&down0_layout, "vs_full"),
                        Pipe::Down => (&down_layout, "vs_full"),
                        Pipe::Wave => (&wave_layout, "vs"),
                        _ => (&gfx_layout, "vs"),
                    };
                    let t0 = std::time::Instant::now();
                    let built = build_pipe(&device, module, layout, vertex, pipe, target);
                    if std::env::var_os("GLINT_PIPE_TIMES").is_some() {
                        eprintln!("PIPE {:?} {:?} {:.0} ms", pipe, target, t0.elapsed().as_secs_f64() * 1000.0);
                    }
                    if tx.send(((target, pipe), built)).is_err() {
                        return;
                    }
                }
            });
            if spawned.is_ok() {
                self.warm.push(rx);
            }
        }
    }

    pub fn ensure_pipe(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat, pipe: Pipe) {
        self.ensure_pipes(device, format, &[pipe]);
    }

    pub fn ensure_glass(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        self.ensure_pipes(device, format, &[Pipe::Glass, Pipe::Veil]);
    }

    /// Whether every pipeline and compute resource asked for so far is built.
    pub fn ready(&self) -> bool {
        self.warm.is_empty() && self.wave_building.is_none()
    }

    /// Whether glass and progressive veil pipelines are built.
    pub fn glass_ready(&self, format: wgpu::TextureFormat) -> bool {
        self.pipes.contains_key(&(format, Pipe::Glass))
            && self.pipes.contains_key(&(format, Pipe::Veil))
            && self.pipes.contains_key(&(PYR_FMT, Pipe::Down0))
            && self.pipes.contains_key(&(PYR_FMT, Pipe::Down))
    }

    pub fn wave_ready(&self, format: wgpu::TextureFormat) -> bool {
        self.pipes.contains_key(&(format, Pipe::Wave)) && self.wave_field.is_some()
    }

    pub fn stage_ready(&self, format: wgpu::TextureFormat) -> bool {
        self.pipes.contains_key(&(format, Pipe::Stage))
    }

    pub fn bars_ready(&self, format: wgpu::TextureFormat) -> bool {
        self.pipes.contains_key(&(format, Pipe::Bars))
    }

    pub fn mood_ready(&self, format: wgpu::TextureFormat) -> bool {
        self.pipes.contains_key(&(format, Pipe::Mood))
    }

    pub fn ribbon_ready(&self, format: wgpu::TextureFormat) -> bool {
        self.pipes.contains_key(&(format, Pipe::Ribbon))
    }

    /// Whether the pipeline a slot drawn into a target of `format` needs is built yet.
    pub fn has_pipe(&mut self, format: wgpu::TextureFormat, pipe: Pipe) -> bool {
        self.take_warm();
        self.pipes.contains_key(&(format, pipe))
    }

    /// Ensures the offscreen-layer (`Rgba16Float`) variant of `pipe` is built on first use instead
    /// of eagerly compiling eight extra pipelines at startup.
    pub fn ensure_layer_pipe(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat, pipe: Pipe) -> bool {
        self.take_warm();
        if self.pipes.contains_key(&(format, pipe)) {
            return true;
        }
        if format != LAYER || !LAYER_PIPES.contains(&pipe) {
            return false;
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(pipe.entry()), source: wgpu::ShaderSource::Wgsl(shader_source(pipe).into()) });
        let layout = if pipe == Pipe::Wave { &self.wave_layout } else { &self.gfx_layout };
        let built = build_pipe(device, &module, layout, "vs", pipe, format);
        self.pipes.insert((format, pipe), built);
        true
    }

    /// Takes in whatever the workers have finished.
    pub(super) fn take_warm(&mut self) {
        let pipes = &mut self.pipes;
        self.warm.retain(|rx| {
            loop {
                match rx.try_recv() {
                    Ok((k, p)) => {
                        pipes.entry(k).or_insert(p);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break true,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break false,
                }
            }
        });
    }
}
