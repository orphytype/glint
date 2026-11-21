//! The GPU side of the compositor: the pipelines for the per-effect WGSL shaders and `down.wgsl`,
//! a pool of uniform buffers, the blur pyramids and the kept frame.

mod pipes;
mod resources;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytemuck::{Pod, Zeroable};
use rune::compose::DeviceKey;

pub use pipes::Pipe;
pub use resources::Region;

/// The pyramids are kept in half floats whatever the frame is, so a dark blur never bands.
pub const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Frames a texture bind group is kept after its last draw. Short: it keeps its textures alive.
const BIND_KEEP: u64 = 4;

/// The one uniform block every shader reads (`common.wgsl`, `U`).
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

pub(super) struct UBuf {
    pub(super) buf: wgpu::Buffer,
    pub(super) bind: wgpu::BindGroup,
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
    /// Bind group layout for `Down0` (reads backdrop with bilinear sampler).
    d0_layout: wgpu::BindGroupLayout,
    /// Bind group layout for `Down` (uses `textureLoad` only, avoiding D3D12 sampler heap waste).
    d_layout: wgpu::BindGroupLayout,
    gfx_layout: wgpu::PipelineLayout,
    down0_layout: wgpu::PipelineLayout,
    down_layout: wgpu::PipelineLayout,
    pipes: HashMap<(wgpu::TextureFormat, Pipe), wgpu::RenderPipeline>,
    /// Pipelines being built on worker threads (see `ensure`), one channel per worker.
    warm: Vec<std::sync::mpsc::Receiver<((wgpu::TextureFormat, Pipe), wgpu::RenderPipeline)>>,
    /// The target formats whose pipelines have been asked for.
    started: Vec<wgpu::TextureFormat>,
    sampler: wgpu::Sampler,
    dummy: wgpu::TextureView,
    /// The current playing cover (40x40), for `Pipe::Stage`.
    art: Option<wgpu::TextureView>,
    /// The live readings of the stage (for `fs_stage`): one row of texels, plus the last uploaded
    /// half-float payload so unchanged frames skip `write_texture`.
    live: Option<(wgpu::Texture, wgpu::TextureView)>,
    live_data: [u16; 44],
    live_valid: bool,
    /// The playing track's waveform for the seek line (`fs_bars`): two rows, four values per texel.
    wave: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// What each uniform buffer of this frame holds (for `upload_in_layer`).
    shadow: Vec<U>,
    /// The current and previous covers, sharper, for My Wave's cover fill (`Pipe::Wave`).
    sharp: [Option<wgpu::TextureView>; 2],
    art_id: usize,
    /// The picture behind the player (with its smaller copies), its id, and whether it is active.
    backdrop: Option<(u64, wgpu::TextureView)>,
    backdrop_on: bool,
    ubufs: Vec<UBuf>,
    used: usize,
    /// Small cache of `(bd, pyr)` texture bind groups (typically 4-15 active pairs per frame).
    /// Keeping this flat avoids per-draw mutex locks, hashing and `TextureView`/`BindGroup` clones.
    binds: Vec<(wgpu::TextureView, wgpu::TextureView, wgpu::BindGroup, u64)>,
    /// The same for the first pass of a pyramid (`Down0`), by the backdrop view it reads.
    first_binds: Vec<(wgpu::TextureView, wgpu::BindGroup, u64)>,
    pyrs: HashMap<(u32, u32, u8), resources::Pyr>,
    frame: u64,
    size: [u32; 2],
    /// Frames handed to the queue and how many have finished on the GPU.
    submitted: u64,
    completed: Arc<AtomicU64>,
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
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(size_of::<U>() as u64) },
                count: None,
            }],
        });
        let t_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("glint textures"), entries: &[tex(0), tex(1), smp(2)] });
        let d0_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("glint pyramid0"), entries: &[tex(0), smp(1)] });
        let d_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("glint pyramid"), entries: &[tex(0)] });
        let gfx_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("glint gfx"), bind_group_layouts: &[Some(&u_layout), Some(&t_layout)], immediate_size: 0 });
        let down0_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glint down0"),
            bind_group_layouts: &[Some(&u_layout), Some(&d0_layout)],
            immediate_size: 0,
        });
        let down_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glint down"),
            bind_group_layouts: &[Some(&u_layout), Some(&d_layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glint linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
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
            d0_layout,
            d_layout,
            gfx_layout,
            down0_layout,
            down_layout,
            pipes: HashMap::new(),
            warm: Vec::new(),
            started: Vec::new(),
            sampler,
            dummy,
            art: None,
            live: None,
            live_data: [0; 44],
            live_valid: false,
            wave: None,
            shadow: Vec::new(),
            sharp: [None, None],
            art_id: 0,
            backdrop: None,
            backdrop_on: false,
            ubufs: Vec::new(),
            used: 0,
            binds: Vec::new(),
            first_binds: Vec::new(),
            pyrs: HashMap::new(),
            frame: 0,
            size: [0, 0],
            submitted: 0,
            completed: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn begin(&mut self) {
        self.frame += 1;
        self.used = 0;
    }

    /// Reclaims stale blur pyramids and texture bind groups, and caps GPU submissions in flight
    /// so D3D12 command allocators and staging memory do not pile up under DWM throttling.
    pub fn finish(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2]) {
        let frame = self.frame;
        let keep = if self.size == size { BIND_KEEP } else { 1 };
        self.size = size;
        self.pyrs.retain(|_, p| frame - p.used < keep);
        self.binds.retain(|(_, _, _, used)| frame - *used < keep);
        self.first_binds.retain(|(_, _, used)| frame - *used < keep);
        self.submitted += 1;
        let done = self.completed.clone();
        queue.on_submitted_work_done(move || {
            done.fetch_add(1, Ordering::Release);
        });
        let limit = 2;
        if self.submitted.saturating_sub(self.completed.load(Ordering::Acquire)) > limit {
            let _ = device.poll(wgpu::PollType::Poll);
            for _ in 0..20 {
                if self.submitted.saturating_sub(self.completed.load(Ordering::Acquire)) <= limit {
                    break;
                }
                let wait = wgpu::PollType::Wait { submission_index: None, timeout: Some(std::time::Duration::from_millis(50)) };
                match device.poll(wait) {
                    Ok(_) | Err(wgpu::PollError::Timeout) => {}
                    Err(_) => break,
                }
            }
        }
    }

    /// Number of uniform blocks staged for this frame.
    pub fn used(&self) -> usize {
        self.used
    }

    /// Buffer and staged bytes for uniform slot `i` (`0..self.used()`).
    pub fn uniform(&self, i: usize) -> (&wgpu::Buffer, &U) {
        (&self.ubufs[i].buf, &self.shadow[i])
    }

    /// Moves uniform block `u` into an isolated layer's pass at `origin` in `size` device pixels.
    pub fn upload_in_layer(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, u: usize, origin: [f32; 2], size: [u32; 2]) -> usize {
        let mut v = self.shadow.get(u).copied().unwrap_or_default();
        v.rect[0] -= origin[0];
        v.rect[1] -= origin[1];
        v.bounds = [v.bounds[0] - origin[0], v.bounds[1] - origin[1], v.bounds[2] - origin[0], v.bounds[3] - origin[1]];
        v.misc[0] = origin[0];
        v.misc[1] = origin[1];
        v.view[0] = size[0] as f32;
        v.view[1] = size[1] as f32;
        let slot = self.stage(device, &v);
        queue.write_buffer(&self.ubufs[slot].buf, 0, bytemuck::bytes_of(&v));
        slot
    }

    /// Reserves the next uniform buffer and records `u` to be written before passes begin.
    pub fn stage(&mut self, device: &wgpu::Device, u: &U) -> usize {
        if self.shadow.len() <= self.used {
            self.shadow.resize(self.used + 1, U::default());
        }
        self.shadow[self.used] = *u;
        if self.used == self.ubufs.len() {
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("glint uniforms"),
                size: size_of::<U>() as u64,
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
        self.used += 1;
        self.used - 1
    }

    /// Records one draw of `pipe` into the open pass.
    pub fn draw(&mut self, device: &wgpu::Device, pass: &mut wgpu::RenderPass<'_>, format: wgpu::TextureFormat, pipe: Pipe, u: usize, b: Binds<'_>) {
        let Some(pipeline) = self.pipes.get(&(format, pipe)) else { return };
        let (bd, pyr): (&wgpu::TextureView, &wgpu::TextureView) = match pipe {
            Pipe::Stage => {
                let cur = if self.backdrop_on { self.backdrop.as_ref().map(|x| &x.1) } else { None }.or(self.art.as_ref()).unwrap_or(&self.dummy);
                let live = self.live.as_ref().map_or(&self.dummy, |h| &h.1);
                (cur, live)
            }
            Pipe::Bars => (&self.dummy, self.wave.as_ref().map_or(&self.dummy, |h| &h.1)),
            Pipe::Wave => {
                let cur = self.sharp[0].as_ref().unwrap_or(&self.dummy);
                let prev = self.sharp[1].as_ref().unwrap_or(cur);
                (cur, prev)
            }
            Pipe::Glass | Pipe::Veil => {
                let level = b.pyr.and_then(|d| self.pyrs.get(&(d[0], d[1], b.tag))).map_or(&self.dummy, |p| &p.full);
                (b.bd.unwrap_or(&self.dummy), level)
            }
            Pipe::Field | Pipe::Morph | Pipe::Mood | Pipe::Ribbon | Pipe::Butterfly | Pipe::Down0 | Pipe::Down => (&self.dummy, &self.dummy),
        };
        let bind_idx = if let Some(idx) = self.binds.iter().position(|(a, c, _, _)| a == bd && c == pyr) {
            self.binds[idx].3 = self.frame;
            idx
        } else {
            let view = |binding, v| wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(v) };
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glint textures"),
                layout: &self.t_layout,
                entries: &[view(0, bd), view(1, pyr), wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
            });
            self.binds.push((bd.clone(), pyr.clone(), group, self.frame));
            self.binds.len() - 1
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.ubufs[u].bind, &[]);
        pass.set_bind_group(1, &self.binds[bind_idx].2, &[]);
        pass.draw(0..4, 0..1);
    }
}
