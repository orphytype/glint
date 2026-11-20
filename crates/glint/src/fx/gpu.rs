//! The GPU side of the compositor: the pipelines for `gfx.wgsl` and `down.wgsl`, a pool of
//! uniform buffers, the blur pyramids and the kept frame.
//!
//! RUNE hands a slot a copy of the frame so far (its "backdrop"). For glass and the veil,
//! `Gpu::pyramid` blurs that copy into a chain of ever smaller levels, and the slot's shader reads
//! between two of them to get any blur radius from one set of passes.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use rune::compose::DeviceKey;
use rune::render::CLIP_WGSL;

/// The pyramids are kept in half floats whatever the frame is, so a dark blur never bands.
pub const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const MAX_LEVELS: u32 = 7;

/// The one uniform block every shader reads (`common.wgsl`, `U`). Each effect reads the few
/// fields it needs; see the shader for what they mean there.
#[repr(C)]
#[derive(Clone, Copy, Default, Pod, Zeroable)]
pub struct U {
    pub rect: [f32; 4],
    pub bounds: [f32; 4],
    pub clip: [f32; 4],
    pub fade: [u32; 4],
    pub radii: [f32; 4],
    pub region: [f32; 4],
    pub view: [f32; 4],
    pub misc: [f32; 4],
    pub p0: [f32; 4],
    pub p1: [f32; 4],
    pub p2: [f32; 4],
    pub p3: [f32; 4],
    pub tint: [f32; 4],
    pub ink: [f32; 4],
    pub p4: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pipe {
    Field,
    Glass,
    Veil,
    Morph,
    /// The seek line.
    Bars,
    /// The cover breaking into blocks and settling into the next one when the track changes
    /// (`fs_artmorph`); it reads the two sharper cover textures.
    ArtMorph,
    /// Behind the open player (`fs_stage`): it reads the playing cover and the live readings of
    /// the music (`Gpu::set_live`).
    Stage,
    /// An artist's picture in its shape (`fs_shape`), and any other plain shape.
    Shape,
    /// My Wave's liquid and the moods' pebbles and pictures (`fs_liquid`); it reads the sharper
    /// playing cover, for the one filled with it.
    Liquid,
    /// A wave card's ribbons (`fs_ribbon`).
    Ribbon,
    /// A butterfly (`fs_butterfly`).
    Butterfly,
    /// Copies a texture over the frame (the kept frame, shown again).
    Blit,
    /// The slot where the frame is copied for every blur of it; it draws nothing.
    Snap,
    /// The first pyramid level, and every one after it.
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
            Pipe::ArtMorph => "fs_artmorph",
            Pipe::Shape => "fs_shape",
            Pipe::Liquid => "fs_liquid",
            Pipe::Ribbon => "fs_ribbon",
            Pipe::Butterfly => "fs_butterfly",
            Pipe::Stage => "fs_stage",
            Pipe::Blit | Pipe::Snap => "fs_blit",
            Pipe::Down0 => "fs_down0",
            Pipe::Down => "fs_down",
        }
    }

    fn is_down(self) -> bool {
        matches!(self, Pipe::Down0 | Pipe::Down)
    }

    /// Pipelines that copy instead of blending over the frame.
    fn opaque(self) -> bool {
        matches!(self, Pipe::Blit | Pipe::Down0 | Pipe::Down)
    }
}

/// The format of RUNE's layers, and the slots that can be drawn into one (all but the ones that
/// read the frame behind them).
const LAYER: wgpu::TextureFormat = HDR;
const LAYER_PIPES: [Pipe; 9] = [Pipe::Field, Pipe::Morph, Pipe::Bars, Pipe::ArtMorph, Pipe::Shape, Pipe::Liquid, Pipe::Ribbon, Pipe::Butterfly, Pipe::Stage];

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

/// Where a pyramid sits: a rectangle of the pass whose size is a multiple of two, so that level 0
/// is exactly half of it, and how many levels that supports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    /// x0, y0, x1, y1 in pass pixels.
    pub rect: [f32; 4],
    /// Level 0's size in texels.
    pub dims: [u32; 2],
    pub levels: u32,
}

impl Region {
    /// The part of a `pass`-sized target inside `want` (x0, y0, x1, y1) that a pyramid covers,
    /// or `None` when it is too small to be worth one.
    ///
    /// `sigma` is the widest blur (in pixels) the slot will ask of it: levels beyond what that
    /// needs are never read, so they are not made.
    pub fn plan(want: [f32; 4], pass: [u32; 2], sigma: f32) -> Option<Region> {
        let x0 = want[0].floor().clamp(0.0, pass[0] as f32);
        let y0 = want[1].floor().clamp(0.0, pass[1] as f32);
        let x1 = want[2].ceil().clamp(0.0, pass[0] as f32);
        let y1 = want[3].ceil().clamp(0.0, pass[1] as f32);
        let (w, h) = (x1 - x0, y1 - y0);
        if w < 8.0 || h < 8.0 {
            return None;
        }
        let small = ((w / 2.0).ceil() as u32).min((h / 2.0).ceil() as u32);
        let by_size = (u32::BITS - small.leading_zeros()).clamp(1, MAX_LEVELS);
        // A blur of `sigma` reads level log2(sigma / 1.58) and the one above it.
        let needed = (sigma / 1.58).max(1.0).log2().floor() as u32 + 2;
        let levels = by_size.min(needed.max(1));
        // The pyramid's texels sit on a grid as coarse as its last level, fixed to the pass: a
        // pane that moves (or settles, a fraction of a pixel a frame) moves over that grid
        // instead of dragging it along, which would shift every coarse texel by a whole pixel of
        // level 0 each time its edge crossed one and make the blur twitch.
        let grid = (1u32 << levels) as f32;
        let (x0, y0) = ((x0 / grid).floor() * grid, (y0 / grid).floor() * grid);
        let (x1, y1) = ((x1 / grid).ceil() * grid, (y1 / grid).ceil() * grid);
        let dims = [((x1 - x0) / 2.0) as u32, ((y1 - y0) / 2.0) as u32];
        Some(Region { rect: [x0, y0, x1, y1], dims, levels })
    }
}

/// Frames a texture bind group is kept after its last draw. Short: it keeps its textures alive.
const BIND_KEEP: u64 = 4;

struct UBuf {
    buf: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

struct Pyr {
    _tex: wgpu::Texture,
    full: wgpu::TextureView,
    mips: Vec<wgpu::TextureView>,
    /// `binds[k]` samples level `k`, for the pass that writes level `k + 1`.
    binds: Vec<wgpu::BindGroup>,
    used: u64,
    /// Content key of the backdrop used to fill this pyramid.
    source_key: u64,
}

/// A frame-sized copy of what the window showed last.
struct Page {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    format: wgpu::TextureFormat,
}

impl Page {
    fn new(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> Page {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("page"),
            size: wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        Page { tex, view, format }
    }
}

/// What a draw reads besides its uniforms.
#[derive(Default, Clone, Copy)]
pub struct Binds<'a> {
    /// The frame so far.
    pub bd: Option<&'a wgpu::TextureView>,
    /// Level-0 size of the pyramid built from it, and the tag it was made under.
    pub pyr: Option<[u32; 2]>,
    pub tag: u8,
}

pub struct Gpu {
    pub key: DeviceKey,
    u_layout: wgpu::BindGroupLayout,
    t_layout: wgpu::BindGroupLayout,
    d_layout: wgpu::BindGroupLayout,
    gfx_layout: wgpu::PipelineLayout,
    down_layout: wgpu::PipelineLayout,
    gfx: wgpu::ShaderModule,
    down: wgpu::ShaderModule,
    pipes: HashMap<(wgpu::TextureFormat, Pipe), wgpu::RenderPipeline>,
    /// Pipelines being built on worker threads (see `ensure`), one channel per worker.
    warm: Vec<std::sync::mpsc::Receiver<((wgpu::TextureFormat, Pipe), wgpu::RenderPipeline)>>,
    /// The target formats whose pipelines have been asked for.
    started: Vec<wgpu::TextureFormat>,
    sampler: wgpu::Sampler,
    dummy: wgpu::TextureView,
    /// The playing cover and the one before it (for the crossfade), tiny, and which thumbnail the
    /// first one was made from.
    art: [Option<wgpu::TextureView>; 2],
    /// The live readings of the stage (for `fs_stage`): one row of texels.
    live: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// The playing track's waveform for the seek line (`fs_bars`): two rows, the shape now and
    /// the one it grows out of, four values to a texel.
    wave: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// What each uniform buffer of this frame holds (for `upload_in_layer`).
    shadow: Vec<U>,
    /// The same two covers, sharper, for the morph between them (`Pipe::ArtMorph`).
    sharp: [Option<wgpu::TextureView>; 2],
    art_id: usize,
    /// A picture drawn in a shape (`Pipe::Shape` with tag 1: an artist on their page), which
    /// picture it is and the side it was shrunk to.
    pic: Option<(usize, u32, wgpu::TextureView)>,
    /// The picture behind the player (with its smaller copies) and which one it is, and whether
    /// the stage draws it this frame.
    backdrop: Option<(u64, wgpu::TextureView)>,
    backdrop_on: bool,
    ubufs: Vec<UBuf>,
    used: usize,
    /// Texture bind groups made for a pair of views (`tbind`) or one pyramid source (`dbind`), and
    /// the frame that last used each. A bind group with a sampler costs D3D12 a buffer of its own
    /// (64 KiB): made afresh for every draw of every frame, they came and went by the thousand a
    /// second, scattering the GPU heap until it held several times what the frame used.
    binds: std::sync::Mutex<HashMap<(wgpu::TextureView, wgpu::TextureView), (wgpu::BindGroup, u64)>>,
    /// The same for the first pass of a pyramid, by the backdrop it reads.
    first_binds: HashMap<wgpu::TextureView, (wgpu::BindGroup, u64)>,
    pyrs: HashMap<(u32, u32, u8), Pyr>,
    frame: u64,
    size: [u32; 2],
    /// The last frame as it was presented, while the window animates: a frame the screen asks for
    /// before anything has moved is this again.
    shown: Option<Page>,
}

impl Gpu {
    pub fn new(device: &wgpu::Device, key: DeviceKey) -> Gpu {
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let smp = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let u_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glint uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<U>() as u64),
                },
                count: None,
            }],
        });
        let t_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("glint textures"), entries: &[tex(0), tex(1), smp(2)] });
        let d_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("glint pyramid"), entries: &[tex(0), smp(1)] });
        let gfx_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("glint gfx"), bind_group_layouts: &[Some(&u_layout), Some(&t_layout)], immediate_size: 0 });
        let down_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glint down"),
            bind_group_layouts: &[Some(&u_layout), Some(&d_layout)],
            immediate_size: 0,
        });
        let common = include_str!("../shaders/common.wgsl");
        let gfx = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glint gfx"),
            source: wgpu::ShaderSource::Wgsl(format!("{CLIP_WGSL}\n{common}\n{}", include_str!("../shaders/gfx.wgsl")).into()),
        });
        let down = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glint down"),
            source: wgpu::ShaderSource::Wgsl(format!("{common}\n{}", include_str!("../shaders/down.wgsl")).into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glint linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            // Between two of a picture's smaller copies (the picture behind the player is
            // blurred by reading a smaller one); the other textures have only one.
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let dummy = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("glint stand-in"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        Gpu {
            key,
            u_layout,
            t_layout,
            d_layout,
            gfx_layout,
            down_layout,
            gfx,
            down,
            pipes: HashMap::new(),
            warm: Vec::new(),
            started: Vec::new(),
            sampler,
            dummy,
            art: [None, None],
            live: None,
            wave: None,
            shadow: Vec::new(),
            sharp: [None, None],
            art_id: 0,
            pic: None,
            backdrop: None,
            backdrop_on: false,
            ubufs: Vec::new(),
            used: 0,
            binds: std::sync::Mutex::new(HashMap::new()),
            first_binds: HashMap::new(),
            pyrs: HashMap::new(),
            frame: 0,
            size: [0, 0],
            shown: None,
        }
    }

    /// Starts building every pipeline a frame of `format` can use, on worker threads, the first
    /// time one is drawn. Nothing is compiled on the UI thread: built there, they held
    /// the first frame back ~2.5 s (D3D12 compiles each from the whole shader). Until a
    /// pipeline is in, the slots that need it are skipped and the window draws the rest.
    pub fn ensure(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        self.take_warm();
        if self.started.contains(&format) {
            return;
        }
        self.started.push(format);
        // Each pipeline for the frame's format and then for a layer's, one after the other on the
        // same worker: the second is the same shader, which the driver has just compiled, so it
        // costs next to nothing (built side by side, both would be compiled in full). The slowest
        // to compile go first, each on a worker of its own, so the look comes in early.
        let order =
            [Pipe::Liquid, Pipe::Glass, Pipe::Stage, Pipe::Veil, Pipe::Ribbon, Pipe::Butterfly, Pipe::Shape, Pipe::Field, Pipe::ArtMorph, Pipe::Bars, Pipe::Morph, Pipe::Blit];
        let mut jobs: Vec<Vec<(wgpu::TextureFormat, Pipe)>> = order
            .into_iter()
            .map(|p| {
                let mut v = vec![(format, p)];
                if LAYER_PIPES.contains(&p) && LAYER != format {
                    v.push((LAYER, p));
                }
                v
            })
            .chain([vec![(HDR, Pipe::Down0), (HDR, Pipe::Down)]])
            .map(|v| v.into_iter().filter(|k| !self.pipes.contains_key(k)).collect::<Vec<_>>())
            .filter(|v| !v.is_empty())
            .collect();
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8).min(jobs.len().max(1));
        let mut chunks: Vec<Vec<(wgpu::TextureFormat, Pipe)>> = vec![Vec::new(); threads];
        for (i, job) in jobs.drain(..).enumerate() {
            chunks[i % threads].extend(job);
        }
        for chunk in chunks.into_iter().filter(|c| !c.is_empty()) {
            let (tx, rx) = std::sync::mpsc::channel();
            let (device, gfx, down, gfx_layout, down_layout) = (device.clone(), self.gfx.clone(), self.down.clone(), self.gfx_layout.clone(), self.down_layout.clone());
            let spawned = std::thread::Builder::new().name("glint-pipelines".into()).spawn(move || {
                for (target, pipe) in chunk {
                    let (module, layout, vertex) = if pipe.is_down() { (&down, &down_layout, "vs_full") } else { (&gfx, &gfx_layout, "vs") };
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
        // A snapshot's frames come faster than the workers: it waits for them, so the effects
        // are in the picture.
        if crate::app::is_headless() {
            for rx in std::mem::take(&mut self.warm) {
                for (k, p) in rx {
                    self.pipes.entry(k).or_insert(p);
                }
            }
        }
    }

    /// Whether every pipeline asked for so far is built.
    pub fn ready(&self) -> bool {
        self.warm.is_empty()
    }

    /// Whether the pipeline a slot drawn into a target of `format` needs is built yet.
    pub fn ensure_pipe(&mut self, _device: &wgpu::Device, format: wgpu::TextureFormat, pipe: Pipe) -> bool {
        self.take_warm();
        self.pipes.contains_key(&(format, pipe))
    }

    /// Takes in whatever the workers have finished.
    fn take_warm(&mut self) {
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

    /// Starts a frame of `size`: hands every uniform buffer back, drops pyramids nobody has used
    /// for a while, and forgets the kept frame when the window changed size.
    pub fn begin(&mut self, size: [u32; 2]) {
        self.frame += 1;
        self.used = 0;
        let frame = self.frame;
        self.pyrs.retain(|_, p| frame.saturating_sub(p.used) < 90);
        self.binds.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|_, (_, used)| frame.saturating_sub(*used) < BIND_KEEP);
        self.first_binds.retain(|_, (_, used)| frame.saturating_sub(*used) < BIND_KEEP);
        if size != self.size {
            self.size = size;
            self.shown = None;
        }
    }

    /// A reused frame still depends on the cached blur textures. Keep them alive while the
    /// compositor is presenting that frame without recording new backdrop passes.
    pub fn touch_pyramids(&mut self) {
        for pyramid in self.pyrs.values_mut() {
            pyramid.used = self.frame;
        }
    }

    /// Writes `u` into the next free buffer and returns its place. All of a frame's uniforms are
    /// written before it is submitted, so each draw (and each pyramid) needs a buffer of its own.
    /// The uniform block `upload` gave index `u`, moved into an isolated layer's pass (a group
    /// that fades): the same slot drawn at `origin` in a target of `size` device pixels.
    pub fn upload_in_layer(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, u: usize, origin: [f32; 2], size: [u32; 2]) -> usize {
        let mut v = self.shadow.get(u).copied().unwrap_or_default();
        v.rect[0] -= origin[0];
        v.rect[1] -= origin[1];
        v.bounds = [v.bounds[0] - origin[0], v.bounds[1] - origin[1], v.bounds[2] - origin[0], v.bounds[3] - origin[1]];
        v.misc[0] = origin[0];
        v.misc[1] = origin[1];
        v.view[0] = size[0] as f32;
        v.view[1] = size[1] as f32;
        self.upload(device, queue, &v)
    }

    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, u: &U) -> usize {
        if self.shadow.len() <= self.used {
            self.shadow.resize(self.used + 1, U::default());
        }
        self.shadow[self.used] = *u;
        if self.used == self.ubufs.len() {
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("glint uniforms"),
                size: std::mem::size_of::<U>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glint uniforms"),
                layout: &self.u_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
            });
            self.ubufs.push(UBuf { buf, bind });
        }
        queue.write_buffer(&self.ubufs[self.used].buf, 0, bytemuck::bytes_of(u));
        self.used += 1;
        self.used - 1
    }

    /// The bind group of `bd` and `pyr` with the sampler, made the first time the pair is drawn
    /// and kept while draws keep using it.
    fn tbind(&self, device: &wgpu::Device, bd: &wgpu::TextureView, pyr: &wgpu::TextureView) -> wgpu::BindGroup {
        let mut binds = self.binds.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = binds.entry((bd.clone(), pyr.clone())).or_insert_with(|| {
            let view = |binding, v| wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(v) };
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glint textures"),
                layout: &self.t_layout,
                entries: &[view(0, bd), view(1, pyr), wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
            });
            (group, 0)
        });
        entry.1 = self.frame;
        entry.0.clone()
    }

    fn make_pyr(&self, device: &wgpu::Device, region: &Region) -> Pyr {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("blur pyramid"),
            size: wgpu::Extent3d { width: region.dims[0], height: region.dims[1], depth_or_array_layers: 1 },
            mip_level_count: region.levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let full = tex.create_view(&Default::default());
        let mips: Vec<wgpu::TextureView> =
            (0..region.levels).map(|m| tex.create_view(&wgpu::TextureViewDescriptor { base_mip_level: m, mip_level_count: Some(1), ..Default::default() })).collect();
        let binds = (0..region.levels.saturating_sub(1) as usize).map(|k| self.dbind(device, &mips[k])).collect();
        Pyr { _tex: tex, full, mips, binds, used: self.frame, source_key: u64::MAX }
    }

    fn dbind(&self, device: &wgpu::Device, src: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glint pyramid"),
            layout: &self.d_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        })
    }

    /// Blurs `src` (the backdrop) into a pyramid over `region`. `u` is the uniform buffer of the
    /// slot: its `region` and `view` fields say where and how big `src` is. `tag` identifies the
    /// slot and `source_key` says whether the copied backdrop changed since the last build.
    #[allow(clippy::too_many_arguments, reason = "each is a different resource of the same pass")]
    pub fn pyramid(&mut self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, src: &wgpu::TextureView, u: usize, region: &Region, tag: u8, source_key: u64) {
        let key = (region.dims[0], region.dims[1], tag);
        let frame = self.frame;
        if self.pyrs.get(&key).is_some_and(|p| p.mips.len() == region.levels as usize && p.source_key == source_key) {
            if let Some(p) = self.pyrs.get_mut(&key) {
                p.used = frame;
            }
            return;
        }
        // Its passes need their pipelines now, warmed up or not: a pyramid left unwritten
        // would be read as whatever the texture held.
        self.ensure_pipe(device, HDR, Pipe::Down0);
        self.ensure_pipe(device, HDR, Pipe::Down);
        if self.pyrs.get(&key).is_none_or(|p| p.mips.len() != region.levels as usize) {
            // A new size for this slot: the ones it had before are done with, unless another slot
            // with the same tag drew from one this frame. A pane changing size (a dialog opening,
            // a menu growing) used to leave one full pyramid behind for every frame of it, each
            // kept for 90 frames: hundreds of megabytes at the peak, which the GPU heap kept.
            self.pyrs.retain(|k, p| k.2 != tag || p.used == frame);
            let p = self.make_pyr(device, region);
            self.pyrs.insert(key, p);
        }
        let first = match self.first_binds.get_mut(src) {
            Some((group, used)) => {
                *used = frame;
                group.clone()
            }
            None => {
                let group = self.dbind(device, src);
                self.first_binds.insert(src.clone(), (group.clone(), frame));
                group
            }
        };
        let Some(p) = self.pyrs.get_mut(&key) else { return };
        p.used = frame;
        p.source_key = source_key;
        for k in 0..region.levels as usize {
            let (pipe, bind) = if k == 0 { (Pipe::Down0, &first) } else { (Pipe::Down, &p.binds[k - 1]) };
            let Some(pipeline) = self.pipes.get(&(HDR, pipe)) else { return };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blur pyramid"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &p.mips[k],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.ubufs[u].bind, &[]);
            pass.set_bind_group(1, bind, &[]);
            pass.draw(0..4, 0..1);
        }
    }

    /// Keeps the frame just drawn into `target` (call it after the last pass of the frame).
    pub fn keep_frame(&mut self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, format: wgpu::TextureFormat) {
        let src = target.texture();
        let (w, h) = (src.width(), src.height());
        if src.format().remove_srgb_suffix() != format.remove_srgb_suffix() || w == 0 || h == 0 {
            self.shown = None;
            return;
        }
        if self.shown.as_ref().is_none_or(|p| p.format != format || p.tex.width() != w || p.tex.height() != h) {
            self.shown = Some(Page::new(device, [w, h], format));
        }
        let Some(page) = &self.shown else { return };
        enc.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo { texture: src, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyTextureInfo { texture: &page.tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Whether a kept frame is there for a target of `size` and `format`, with the pipeline to show it.
    pub fn has_kept(&self, size: [u32; 2], format: wgpu::TextureFormat) -> bool {
        self.pipes.contains_key(&(format, Pipe::Blit)) && self.shown.as_ref().is_some_and(|p| p.format == format && [p.tex.width(), p.tex.height()] == size)
    }

    /// Draws the kept frame into `target`. `u` is a uniform block whose `bounds` and `view` cover it.
    pub fn show_kept(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, format: wgpu::TextureFormat, u: usize) {
        let (Some(page), Some(pipeline)) = (&self.shown, self.pipes.get(&(format, Pipe::Blit))) else { return };
        let bind = self.tbind(device, &page.view, &self.dummy);
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("same frame"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.ubufs[u].bind, &[]);
        pass.set_bind_group(1, &bind, &[]);
        pass.draw(0..4, 0..1);
    }

    /// The live readings of the stage, one row: eight texels of spectrum (32 bands), then the look
    /// chosen in the player's panel, the beat, the pulse and whether anything moves, then the low
    /// end, the middle, the top and the kick.
    pub fn set_live(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, texels: &[[f32; 4]]) {
        let tex = self.live_tex(device, texels.len() as u32);
        let data: Vec<u16> = texels.iter().flat_map(|t| t.map(half)).collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8 * texels.len() as u32), rows_per_image: Some(1) },
            wgpu::Extent3d { width: texels.len() as u32, height: 1, depth_or_array_layers: 1 },
        );
    }

    /// Hands over a track's waveform (`now`) and the one the seek line changes from (`from`),
    /// each `fx::WAVE_N` values.
    pub fn set_wave(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, now: &[f32], from: &[f32]) {
        let w = (crate::fx::WAVE_N / 4) as u32;
        let (tex, _) = self.wave.get_or_insert_with(|| {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("waveform"),
                size: wgpu::Extent3d { width: w, height: 2, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            (tex, view)
        });
        fn row(v: &[f32]) -> impl Iterator<Item = u16> + '_ {
            (0..crate::fx::WAVE_N).map(move |i| half(v.get(i).copied().unwrap_or(0.0)))
        }
        let data: Vec<u16> = row(now).chain(row(from)).collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8 * w), rows_per_image: Some(2) },
            wgpu::Extent3d { width: w, height: 2, depth_or_array_layers: 1 },
        );
    }

    /// The texture the live readings live in, made once.
    fn live_tex(&mut self, device: &wgpu::Device, n: u32) -> wgpu::Texture {
        let (tex, _) = self.live.get_or_insert_with(|| {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("stage readings"),
                size: wgpu::Extent3d { width: n, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            (tex, view)
        });
        tex.clone()
    }

    /// Hands over the picture a shape is filled with, shrunk to `side` texels a side (about what
    /// it covers on screen, so it is neither soft nor shimmering). The same picture at the same
    /// side is not uploaded twice.
    pub fn set_pic(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, id: usize, img: &crate::art::Decoded, side: u32) {
        let side = side.clamp(16, 1024);
        if self.pic.as_ref().is_some_and(|(i, s, _)| *i == id && *s == side) {
            return;
        }
        // The middle square of the picture, area-averaged to the side.
        let (w, h) = (img.width, img.height);
        let sq = w.min(h);
        if sq == 0 || img.rgba.len() < (w * h * 4) as usize {
            return;
        }
        let (x0, y0) = ((w - sq) / 2, (h - sq) / 2);
        let mut crop = Vec::with_capacity((sq * sq * 4) as usize);
        for y in y0..y0 + sq {
            let i = ((y * w + x0) * 4) as usize;
            crop.extend_from_slice(&img.rgba[i..i + (sq * 4) as usize]);
        }
        let side = side.min(sq);
        let px = if side == sq { crop } else { crate::art::shrink_square(sq, &crop, side) };
        let view = Self::art_texture(device, queue, side, side, &px);
        self.pic = Some((id, side, view));
    }

    pub fn has_pic(&self) -> bool {
        self.pic.is_some()
    }

    /// Hands over the picture behind the player with all its smaller copies (`backdrop::Image`).
    /// The same picture is not uploaded twice.
    pub fn set_backdrop(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, img: &crate::backdrop::Image) {
        if self.backdrop.as_ref().is_some_and(|(id, _)| *id == img.id) || img.levels.is_empty() {
            return;
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("player backdrop"),
            size: wgpu::Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
            mip_level_count: img.levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (m, (w, h, px)) in img.levels.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: m as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                px,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(*h) },
                wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
            );
        }
        self.backdrop = Some((img.id, tex.create_view(&wgpu::TextureViewDescriptor::default())));
    }

    /// Whether the stage draws the picture behind the player this frame (else the cover's colours).
    pub fn use_backdrop(&mut self, on: bool) {
        self.backdrop_on = on && self.backdrop.is_some();
        if !on {
            self.backdrop = None;
        }
    }

    /// Whether a cover has been handed over (`set_art`), so a cover slot has something to show.
    pub fn has_art(&self) -> bool {
        self.art[0].is_some()
    }

    /// Hands over the playing cover (`id` says which thumbnail it is: the same one is not uploaded
    /// twice). The one before it is kept for the crossfade. Returns whether it was new.
    pub fn set_art(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, id: usize, thumb: &crate::art::Thumb) -> bool {
        if id == self.art_id && self.art[0].is_some() {
            return false;
        }
        let view = Self::art_texture(device, queue, thumb.width, thumb.height, &thumb.rgba);
        let sharp = Self::art_texture(device, queue, thumb.sharp_side, thumb.sharp_side, &thumb.sharp);
        let first = self.art[0].is_none();
        self.art[1] = if first { Some(view.clone()) } else { self.art[0].take() };
        self.art[0] = Some(view);
        self.sharp[1] = if first { Some(sharp.clone()) } else { self.sharp[0].take() };
        self.sharp[0] = Some(sharp);
        self.art_id = id;
        !first
    }

    fn art_texture(device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, rgba: &[u8]) -> wgpu::TextureView {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cover"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        tex.create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// Records one draw of `pipe` into the open pass.
    pub fn draw(&self, device: &wgpu::Device, pass: &mut wgpu::RenderPass<'_>, format: wgpu::TextureFormat, pipe: Pipe, u: usize, b: Binds<'_>) {
        let Some(pipeline) = self.pipes.get(&(format, pipe)) else { return };
        let level = |dims: Option<[u32; 2]>, tag: u8| dims.and_then(|d| self.pyrs.get(&(d[0], d[1], tag))).map_or(&self.dummy, |p| &p.full);
        let bind = if pipe == Pipe::Stage {
            let cur = if self.backdrop_on { self.backdrop.as_ref().map(|b| &b.1) } else { None }.or(self.art[0].as_ref()).unwrap_or(&self.dummy);
            self.tbind(device, cur, self.live.as_ref().map_or(&self.dummy, |h| &h.1))
        } else if pipe == Pipe::Bars {
            self.tbind(device, &self.dummy, self.wave.as_ref().map_or(&self.dummy, |h| &h.1))
        } else if pipe == Pipe::Shape && b.tag == 1 {
            let pic = self.pic.as_ref().map_or(&self.dummy, |p| &p.2);
            self.tbind(device, pic, pic)
        } else if matches!(pipe, Pipe::ArtMorph | Pipe::Shape | Pipe::Liquid | Pipe::Ribbon | Pipe::Butterfly) {
            let cur = self.sharp[0].as_ref().unwrap_or(&self.dummy);
            self.tbind(device, cur, self.sharp[1].as_ref().unwrap_or(cur))
        } else {
            self.tbind(device, b.bd.unwrap_or(&self.dummy), level(b.pyr, b.tag))
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.ubufs[u].bind, &[]);
        pass.set_bind_group(1, &bind, &[]);
        pass.draw(0..4, 0..1);
    }
}

/// A float as a half float's bits (values too small for one become zero).
fn half(x: f32) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let e = ((b >> 23) & 0xff) as i32 - 127 + 15;
    if e <= 0 {
        return sign;
    }
    if e >= 31 {
        return sign | 0x7c00;
    }
    sign | ((e as u16) << 10) | (((b & 0x7f_ffff) >> 13) as u16)
}
