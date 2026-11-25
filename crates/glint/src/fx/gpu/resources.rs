//! Blur pyramids and dynamic GPU textures (stage live readings, seek waveform, backdrop, art).

use super::{Gpu, HDR, PYR_FMT, Pipe};

const MAX_LEVELS: u32 = 7;

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
        // Snap texels to a coarse grid fixed to the pass so sliding glass does not shimmer.
        let grid = (1u32 << levels) as f32;
        let (x0, y0) = ((x0 / grid).floor() * grid, (y0 / grid).floor() * grid);
        let (x1, y1) = ((x1 / grid).ceil() * grid, (y1 / grid).ceil() * grid);
        let dims = [((x1 - x0) / 2.0) as u32, ((y1 - y0) / 2.0) as u32];
        Some(Region { rect: [x0, y0, x1, y1], dims, levels })
    }
}

pub(super) struct Pyr {
    _tex: wgpu::Texture,
    pub(super) full: wgpu::TextureView,
    pub(super) mips: Vec<wgpu::TextureView>,
    /// `binds[k]` reads level `k` with `textureLoad` (no sampler), for the pass writing `k + 1`.
    pub(super) binds: Vec<wgpu::BindGroup>,
    pub(super) used: u64,
    /// Content key of the backdrop used to fill this pyramid.
    pub(super) source_key: u64,
}

impl Gpu {
    fn make_pyr(&self, device: &wgpu::Device, region: &Region) -> Pyr {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("blur pyramid"),
            size: wgpu::Extent3d { width: region.dims[0], height: region.dims[1], depth_or_array_layers: 1 },
            mip_level_count: region.levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PYR_FMT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let full = tex.create_view(&Default::default());
        let mips: Vec<wgpu::TextureView> =
            (0..region.levels).map(|m| tex.create_view(&wgpu::TextureViewDescriptor { base_mip_level: m, mip_level_count: Some(1), ..Default::default() })).collect();
        let binds = (0..region.levels.saturating_sub(1) as usize).map(|k| self.dbind_tex(device, &mips[k])).collect();
        Pyr { _tex: tex, full, mips, binds, used: self.frame, source_key: u64::MAX }
    }

    fn dbind_first(&self, device: &wgpu::Device, src: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glint pyramid0"),
            layout: &self.d0_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        })
    }

    fn dbind_tex(&self, device: &wgpu::Device, src: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glint pyramid"),
            layout: &self.d_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) }],
        })
    }

    /// Blurs `src` (the backdrop) into a pyramid over `region`.
    #[expect(clippy::too_many_arguments, reason = "each is a different resource of the same pass")]
    pub fn pyramid(&mut self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, src: &wgpu::TextureView, u: usize, region: &Region, tag: u8, source_key: u64) {
        let key = (region.dims[0], region.dims[1], tag);
        let frame = self.frame;
        if let Some(p) = self.pyrs.get_mut(&key)
            && p.mips.len() == region.levels as usize
            && p.source_key == source_key
        {
            p.used = frame;
            return;
        }
        // Check both pipelines before marking the pyramid's `source_key` as valid.
        if !self.has_pipe(PYR_FMT, Pipe::Down0) || (region.levels > 1 && !self.has_pipe(PYR_FMT, Pipe::Down)) {
            return;
        }
        if self.pyrs.get(&key).is_none_or(|p| p.mips.len() != region.levels as usize) {
            self.pyrs.retain(|k, p| k.2 != tag || p.used == frame);
            let p = self.make_pyr(device, region);
            self.pyrs.insert(key, p);
        }
        let first_idx = if let Some(idx) = self.first_binds.iter().position(|(v, _, _)| v == src) {
            self.first_binds[idx].2 = frame;
            idx
        } else {
            let group = self.dbind_first(device, src);
            self.first_binds.push((src.clone(), group, frame));
            self.first_binds.len() - 1
        };
        let Some(p) = self.pyrs.get_mut(&key) else { return };
        p.used = frame;
        p.source_key = source_key;
        let first_bind = &self.first_binds[first_idx].1;
        for k in 0..region.levels as usize {
            let (pipe, bind) = if k == 0 { (Pipe::Down0, first_bind) } else { (Pipe::Down, &p.binds[k - 1]) };
            let pipeline = &self.pipes[&(PYR_FMT, pipe)];
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
            pass.set_bind_group(0, &self.ubind, &[u as u32 * self.ustride]);
            pass.set_bind_group(1, bind, &[]);
            pass.draw(0..4, 0..1);
        }
    }

    /// Uploads the stage's live readings (skipped when the half-float payload is unchanged).
    pub fn set_live(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, texels: &[[f32; 4]; 11]) {
        let mut data = [0u16; 44];
        for (i, t) in texels.iter().enumerate() {
            for c in 0..4 {
                data[i * 4 + c] = half(t[c]);
            }
        }
        if self.live_valid && self.live_data == data && self.live.is_some() {
            return;
        }
        let n = texels.len() as u32;
        let (tex, _) = self.live.get_or_insert_with(|| {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("stage readings"),
                size: wgpu::Extent3d { width: n, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            (tex, view)
        });
        self.live_data = data;
        self.live_valid = true;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8 * n), rows_per_image: Some(1) },
            wgpu::Extent3d { width: n, height: 1, depth_or_array_layers: 1 },
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
                format: HDR,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            (tex, view)
        });
        let mut data = [0u16; crate::fx::WAVE_N * 2];
        for i in 0..crate::fx::WAVE_N {
            data[i] = half(now.get(i).copied().unwrap_or(0.0));
            data[crate::fx::WAVE_N + i] = half(from.get(i).copied().unwrap_or(0.0));
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8 * w), rows_per_image: Some(2) },
            wgpu::Extent3d { width: w, height: 2, depth_or_array_layers: 1 },
        );
    }

    /// Hands over the picture behind the player with all its smaller copies (`backdrop::Image`).
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
        self.art.is_some()
    }

    /// Hands over the playing cover (`id` says which thumbnail it is: the same one is not uploaded
    /// twice). The previous sharp cover is kept for My Wave's crossfade, and the blurred one fades
    /// from the old cover (`step_art`). Returns whether it was new.
    pub fn set_art(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, id: usize, thumb: &crate::art::Thumb) -> bool {
        if id == self.art_id && self.art.is_some() {
            return false;
        }
        let first = self.art.is_none();
        let same_size = self.art_tex.as_ref().is_some_and(|&(_, w, h)| w == thumb.width && h == thumb.height) && self.art_now.len() == thumb.rgba.len();
        if !first && same_size {
            self.art_from = std::mem::take(&mut self.art_now);
            self.art_now = self.art_from.clone();
            self.art_to = thumb.rgba.clone();
            self.art_t = 0.0;
        } else {
            let (tex, view) = Self::art_texture(device, queue, thumb.width, thumb.height, &thumb.rgba);
            self.art_tex = Some((tex, thumb.width, thumb.height));
            self.art_now = thumb.rgba.clone();
            self.art_t = 1.0;
            self.art = Some(view);
        }
        self.art_id = id;
        !first
    }

    /// Moves the blurred cover on towards the new one, over `ART_FADE` seconds, as the cover
    /// itself crossfades. True while it is still fading.
    pub fn step_art(&mut self, queue: &wgpu::Queue, dt: f32, reduced: bool) -> bool {
        /// How long the picture behind the player takes to become the new cover.
        const ART_FADE: f32 = 0.8;
        if self.art_t >= 1.0 {
            return false;
        }
        let Some((tex, w, h)) = &self.art_tex else { return false };
        self.art_t = if reduced { 1.0 } else { (self.art_t + dt / ART_FADE).min(1.0) };
        let m = self.art_t * self.art_t * (3.0 - 2.0 * self.art_t);
        for ((now, &from), &to) in self.art_now.iter_mut().zip(&self.art_from).zip(&self.art_to) {
            *now = (f32::from(from) + (f32::from(to) - f32::from(from)) * m).round() as u8;
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &self.art_now,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(*h) },
            wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
        );
        self.art_t < 1.0
    }

    fn art_texture(device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, rgba: &[u8]) -> (wgpu::Texture, wgpu::TextureView) {
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
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
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
