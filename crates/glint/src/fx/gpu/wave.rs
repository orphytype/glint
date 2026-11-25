//! One-time GPU compute builder for the 160x152x128 `Rg8Unorm` 3D wave material field.
//!
//! Runs `fit_spatial` + `fit_cycle` from `wave_init.wgsl` against the embedded 463 KiB
//! `wave-basis.bin`, copies the packed result to a resident 3D texture (5.94 MiB VRAM, 0 B CPU),
//! and immediately drops all temporary staging/compute buffers.

const WAVE_BASIS: &[u8] = include_bytes!("../../../assets/wave-basis.bin");
const WAVE_INIT_WGSL: &str = include_str!("../../shaders/wave_init.wgsl");

const WIDTH: u32 = 160;
const HEIGHT: u32 = 152;
const DEPTH: u32 = 128;
const MODES: u32 = 48;
const ROW_PITCH: u32 = 512;

/// Synthesizes the 3D wave material field on the GPU and returns its resident bind group.
pub fn build_wave_field(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout) -> wgpu::BindGroup {
    debug_assert_eq!(WAVE_BASIS.len(), 474_176);

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("glint wave init"),
        source: wgpu::ShaderSource::Wgsl(WAVE_INIT_WGSL.into()),
    });
    let spatial = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("fit_spatial"),
        layout: None,
        module: &module,
        entry_point: Some("fit_spatial"),
        compilation_options: Default::default(),
        cache: None,
    });
    let cycle = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("fit_cycle"),
        layout: None,
        module: &module,
        entry_point: Some("fit_cycle"),
        compilation_options: Default::default(),
        cache: None,
    });

    let field = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("glint wave field 3d"),
        size: wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: DEPTH },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rg8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });

    let basis = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("glint wave basis"),
        size: WAVE_BASIS.len() as u64,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: true,
    });
    if let Ok(mut view) = basis.slice(..).get_mapped_range_mut() {
        view.copy_from_slice(WAVE_BASIS);
    }
    basis.unmap();
    let modes = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("glint wave spatial modes"),
        size: u64::from(WIDTH * HEIGHT * MODES * 4),
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let packed = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("glint wave field upload"),
        size: u64::from(ROW_PITCH * HEIGHT * DEPTH),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let spatial_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glint wave spatial"),
        layout: &spatial.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 3, resource: basis.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: modes.as_entire_binding() },
        ],
    });
    let cycle_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glint wave cycle"),
        layout: &cycle.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 3, resource: basis.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: modes.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 5, resource: packed.as_entire_binding() },
        ],
    });

    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("glint wave init") });
    {
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("glint wave init"), timestamp_writes: None });
        pass.set_pipeline(&spatial);
        pass.set_bind_group(0, &spatial_bg, &[]);
        pass.dispatch_workgroups(WIDTH / 8, HEIGHT / 8, MODES);
        pass.set_pipeline(&cycle);
        pass.set_bind_group(0, &cycle_bg, &[]);
        pass.dispatch_workgroups(WIDTH / 16, HEIGHT / 8, DEPTH);
    }
    enc.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo {
            buffer: &packed,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(ROW_PITCH), rows_per_image: Some(HEIGHT) },
        },
        wgpu::TexelCopyTextureInfo { texture: &field, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: DEPTH },
    );
    queue.submit([enc.finish()]);
    // Release temporary synthesis buffers as soon as the GPU work finishes.
    drop(spatial_bg);
    drop(cycle_bg);
    drop(basis);
    drop(modes);
    drop(packed);
    drop(spatial);
    drop(cycle);

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("glint wave 3d linear"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let view = field.create_view(&Default::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glint wave 3d"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    })
}
