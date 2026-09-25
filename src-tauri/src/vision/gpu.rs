// ============================================================
// Synapse — GPU Template Matching (wgpu compute)
// ============================================================
// Runs the NCC search (`ncc.wgsl`) on the GPU. The screen is
// uploaded once as packed BGRA, every candidate position is scored
// by its own invocation, and each 16×16 workgroup reduces to a
// single winner. The host reads back only those winners and
// re-scores the top ones exactly on the CPU (f64), so the returned
// position and confidence follow the CPU matcher's semantics.
//
// Large searches are split into several submissions so that no
// single one runs long enough to trip the Windows GPU watchdog.
//
// Environment:
//   SYNAPSE_MATCHER=auto|gpu|cpu   auto (default) uses the GPU for
//                                  large searches only
//   SYNAPSE_GPU_ALLOW_SOFTWARE=1   accept software adapters
//                                  (WARP, llvmpipe) — for testing
// ============================================================

use super::capture::ScreenBuffer;
use super::ncc::{self, NccMatch, PreparedTemplate};
use std::borrow::Cow;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use wgpu::util::DeviceExt;

/// Workgroup edge length; must match `@workgroup_size` in ncc.wgsl
const WG_EDGE: u32 = 16;
/// Screen-pixel reads per submission (each position reads the patch twice)
const DEFAULT_CHUNK_BUDGET: u64 = 1 << 30;
/// Below this many pixel comparisons the CPU beats upload + dispatch + readback
const GPU_MIN_WORK: u64 = 1 << 24;
/// Workgroup winners this close to the GPU best are re-scored on the CPU
const RESCORE_EPSILON: f32 = 1e-3;
/// Upper bound on candidates re-scored on the CPU
const MAX_RESCORE: usize = 64;
/// Consecutive GPU failures before the matcher stops being used
const MAX_FAILURES: u32 = 3;

/// Uniform block; layout must match `Params` in ncc.wgsl
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    screen_w: u32,
    search_w: u32,
    row_offset: u32,
    row_end: u32,
    tmpl_w: u32,
    tmpl_h: u32,
    result_offset: u32,
    _pad0: u32,
    inv_n: f32,
    tmpl_norm: f32,
    _pad1: f32,
    _pad2: f32,
}

/// One submission: candidate rows `[row_offset, row_end)`
struct Chunk {
    row_offset: u32,
    row_end: u32,
    result_offset: u32,
    groups_y: u32,
}

pub struct GpuMatcher {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_layout: wgpu::BindGroupLayout,
    adapter_name: String,
    chunk_budget: u64,
    failures: AtomicU32,
    /// Serializes matching; every run allocates screen-sized buffers
    busy: Mutex<()>,
}

impl GpuMatcher {
    /// Open a GPU device and build the NCC pipeline.
    /// Software adapters are rejected unless `allow_software` is set:
    /// on those the parallel CPU matcher is faster.
    pub fn new(allow_software: bool) -> Result<Self, String> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(desc.with_env());

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| format!("No GPU adapter available: {e}"))?;

        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu && !allow_software {
            return Err(format!("Only a software adapter is available ({})", info.name));
        }
        if !adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        {
            return Err(format!("Adapter '{}' does not support compute shaders", info.name));
        }

        let limits = adapter.limits();
        if limits.max_compute_invocations_per_workgroup < WG_EDGE * WG_EDGE
            || limits.max_compute_workgroup_size_x < WG_EDGE
            || limits.max_compute_workgroup_size_y < WG_EDGE
            || limits.max_compute_workgroup_storage_size < WG_EDGE * WG_EDGE * 8
        {
            return Err(format!("Adapter '{}' compute limits are too small", info.name));
        }

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("synapse-ncc"),
            required_limits: limits,
            ..Default::default()
        }))
        .map_err(|e| format!("Failed to open GPU device: {e}"))?;

        // wgpu panics on uncaptured errors by default; log instead so a driver
        // problem degrades to the CPU matcher rather than crashing the app.
        device.on_uncaptured_error(Arc::new(|e| tracing::error!("wgpu error: {e}")));

        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ncc.wgsl"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("ncc.wgsl"))),
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ncc-bindings"),
            entries: &[
                buffer_entry(0, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_entry(2, wgpu::BufferBindingType::Storage { read_only: false }),
                buffer_entry(3, wgpu::BufferBindingType::Uniform),
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ncc-layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("ncc"),
            layout: Some(&layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        if let Some(err) = pollster::block_on(scope.pop()) {
            return Err(format!("GPU pipeline creation failed: {err}"));
        }

        Ok(Self {
            device,
            queue,
            pipeline,
            bind_layout,
            adapter_name: format!("{} ({:?})", info.name, info.backend),
            chunk_budget: DEFAULT_CHUNK_BUDGET,
            failures: AtomicU32::new(0),
            busy: Mutex::new(()),
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// False once the GPU has failed repeatedly (e.g. device lost)
    pub fn is_healthy(&self) -> bool {
        self.failures.load(Ordering::Relaxed) < MAX_FAILURES
    }

    /// Find the best template position; `Ok(None)` if the template does
    /// not fit. Errors mean the caller should fall back to the CPU.
    pub fn find_best(
        &self,
        screen: &ScreenBuffer,
        tmpl: &PreparedTemplate,
    ) -> Result<Option<NccMatch>, String> {
        let result = self.run(screen, tmpl);
        match &result {
            Ok(_) => self.failures.store(0, Ordering::Relaxed),
            Err(_) => {
                self.failures.fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    }

    fn run(&self, screen: &ScreenBuffer, tmpl: &PreparedTemplate) -> Result<Option<NccMatch>, String> {
        let Some((search_w, search_h)) = ncc::search_dims(screen, tmpl) else {
            return Ok(None);
        };
        let _busy = self.busy.lock().unwrap_or_else(|p| p.into_inner());
        let limits = self.device.limits();

        let groups_x = search_w.div_ceil(WG_EDGE);
        if groups_x > limits.max_compute_workgroups_per_dimension {
            return Err("Search area is too wide for a single GPU dispatch".into());
        }

        let n = tmpl.pixel_count() as u64;
        let chunks = plan_chunks(
            search_w,
            search_h,
            n,
            self.chunk_budget,
            limits.max_compute_workgroups_per_dimension,
        );
        let slots: u64 = chunks
            .iter()
            .map(|c| groups_x as u64 * c.groups_y as u64)
            .sum();
        let results_size = slots * 8;

        let pixels = packed_pixels(screen);
        let max_binding = limits.max_storage_buffer_binding_size.min(limits.max_buffer_size);
        if pixels.len() as u64 > max_binding || results_size > max_binding {
            return Err("Screen region is too large for GPU buffers".into());
        }

        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);

        let screen_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ncc-screen"),
            contents: &pixels,
            usage: wgpu::BufferUsages::STORAGE,
        });

        let tmpl_data: Vec<[f32; 4]> = tmpl
            .centered
            .iter()
            .map(|c| {
                [
                    (c[0] / 255.0) as f32,
                    (c[1] / 255.0) as f32,
                    (c[2] / 255.0) as f32,
                    0.0,
                ]
            })
            .collect();
        let tmpl_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ncc-template"),
            contents: bytemuck::cast_slice(&tmpl_data),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let results_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ncc-results"),
            size: results_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ncc-readback"),
            size: results_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ncc-params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ncc-bind-group"),
            layout: &self.bind_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: screen_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: tmpl_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: results_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
            ],
        });

        for (i, chunk) in chunks.iter().enumerate() {
            let params = Params {
                screen_w: screen.width,
                search_w,
                row_offset: chunk.row_offset,
                row_end: chunk.row_end,
                tmpl_w: tmpl.width,
                tmpl_h: tmpl.height,
                result_offset: chunk.result_offset,
                _pad0: 0,
                inv_n: 1.0 / n as f32,
                tmpl_norm: (tmpl.norm / 255.0) as f32,
                _pad1: 0.0,
                _pad2: 0.0,
            };
            // Applied at the start of the next submit, in queue order, so
            // each chunk sees its own parameters.
            self.queue.write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ncc") });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("ncc"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(groups_x, chunk.groups_y, 1);
            }
            if i + 1 == chunks.len() {
                encoder.copy_buffer_to_buffer(&results_buf, 0, &readback, 0, results_size);
            }
            self.queue.submit([encoder.finish()]);
        }

        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| format!("GPU wait failed: {e}"))?;
        rx.recv()
            .map_err(|_| "GPU readback was cancelled".to_string())?
            .map_err(|e| format!("GPU readback failed: {e}"))?;

        let candidates = {
            let view = slice
                .get_mapped_range()
                .map_err(|e| format!("GPU readback mapping failed: {e}"))?;
            collect_candidates(bytemuck::cast_slice(&view), search_w)
        };
        readback.unmap();

        if let Some(err) = pollster::block_on(scope.pop()) {
            return Err(format!("GPU matching failed: {err}"));
        }
        if candidates.is_empty() {
            return Err("GPU produced no candidates".into());
        }

        Ok(ncc::best_of_candidates(screen, tmpl, candidates))
    }
}

fn buffer_entry(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Split the candidate rows into submissions of at most `budget` pixel
/// reads each (two reads per template pixel per position)
fn plan_chunks(search_w: u32, search_h: u32, n: u64, budget: u64, max_groups: u32) -> Vec<Chunk> {
    let per_row = (search_w as u64 * n * 2).max(1);
    let mut rows = (budget / per_row).max(1);
    if rows >= WG_EDGE as u64 {
        rows -= rows % WG_EDGE as u64; // whole workgroups
    }
    let rows = rows
        .min(max_groups as u64 * WG_EDGE as u64)
        .min(search_h as u64) as u32;

    let groups_x = search_w.div_ceil(WG_EDGE);
    let mut chunks = Vec::new();
    let (mut row, mut slot) = (0u32, 0u32);
    while row < search_h {
        let row_end = (row + rows).min(search_h);
        let groups_y = (row_end - row).div_ceil(WG_EDGE);
        chunks.push(Chunk {
            row_offset: row,
            row_end,
            result_offset: slot,
            groups_y,
        });
        slot += groups_x * groups_y;
        row = row_end;
    }
    chunks
}

/// Screen pixels as tightly packed rows (the shader assumes stride = width)
fn packed_pixels(screen: &ScreenBuffer) -> Cow<'_, [u8]> {
    let row_bytes = screen.width as usize * 4;
    let stride = screen.stride as usize;
    if stride == row_bytes {
        Cow::Borrowed(&screen.data[..row_bytes * screen.height as usize])
    } else {
        Cow::Owned(
            (0..screen.height as usize)
                .flat_map(|y| &screen.data[y * stride..y * stride + row_bytes])
                .copied()
                .collect(),
        )
    }
}

/// Positions of the workgroup winners worth re-scoring exactly
fn collect_candidates(pairs: &[[u32; 2]], search_w: u32) -> Vec<(u32, u32)> {
    let mut winners: Vec<(f32, u32)> = pairs
        .iter()
        .filter(|p| p[1] != u32::MAX)
        .map(|p| (f32::from_bits(p[0]), p[1]))
        .filter(|(score, _)| *score >= 0.0) // also drops NaN
        .collect();

    let Some(top) = winners.iter().map(|w| w.0).reduce(f32::max) else {
        return Vec::new();
    };
    winners.retain(|w| w.0 >= top - RESCORE_EPSILON);
    winners.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    winners.truncate(MAX_RESCORE);

    winners
        .into_iter()
        .map(|(_, idx)| (idx % search_w, idx / search_w))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Auto,
    Gpu,
    Cpu,
}

fn mode() -> Mode {
    match std::env::var("SYNAPSE_MATCHER")
        .map(|v| v.to_ascii_lowercase())
        .as_deref()
    {
        Ok("cpu") => Mode::Cpu,
        Ok("gpu") => Mode::Gpu,
        _ => Mode::Auto,
    }
}

/// Whether a search of `work` pixel comparisons should run on the GPU
pub fn should_use(work: u64) -> bool {
    match mode() {
        Mode::Cpu => false,
        Mode::Gpu => true,
        Mode::Auto => work >= GPU_MIN_WORK,
    }
}

/// Process-wide GPU matcher, created on first use. `None` when no suitable
/// GPU is available, when disabled via `SYNAPSE_MATCHER=cpu`, or after
/// repeated failures.
pub fn global() -> Option<&'static GpuMatcher> {
    static GPU: OnceLock<Option<GpuMatcher>> = OnceLock::new();
    GPU.get_or_init(|| {
        if mode() == Mode::Cpu {
            tracing::info!("GPU template matching disabled (SYNAPSE_MATCHER=cpu)");
            return None;
        }
        let allow_software = std::env::var("SYNAPSE_GPU_ALLOW_SOFTWARE").is_ok_and(|v| v == "1");
        match GpuMatcher::new(allow_software) {
            Ok(m) => {
                tracing::info!("GPU template matching ready on {}", m.adapter_name());
                Some(m)
            }
            Err(e) => {
                tracing::warn!("GPU template matching unavailable, using CPU: {e}");
                None
            }
        }
    })
    .as_ref()
    .filter(|m| m.is_healthy())
}

#[cfg(test)]
mod tests {
    use super::super::ncc::test_util::*;
    use super::*;

    /// A software adapter (llvmpipe/WARP) is fine for correctness tests.
    /// Set SYNAPSE_REQUIRE_GPU=1 to fail instead of skipping without one.
    fn test_matcher() -> Option<GpuMatcher> {
        match GpuMatcher::new(true) {
            Ok(m) => Some(m),
            Err(e) => {
                if std::env::var("SYNAPSE_REQUIRE_GPU").is_ok_and(|v| v == "1") {
                    panic!("GPU required but unavailable: {e}");
                }
                eprintln!("skipping GPU test: {e}");
                None
            }
        }
    }

    fn assert_same_as_cpu(gpu: &GpuMatcher, screen: &ScreenBuffer, raw: &ScreenBuffer) -> NccMatch {
        let tmpl = PreparedTemplate::new(raw).unwrap();
        let cpu = ncc::find_best_cpu(screen, &tmpl).unwrap();
        let got = gpu.find_best(screen, &tmpl).unwrap().unwrap();
        assert_eq!((got.x, got.y), (cpu.x, cpu.y), "gpu {got:?} vs cpu {cpu:?}");
        assert_eq!(got.score, cpu.score);
        got
    }

    #[test]
    fn gpu_finds_embedded_templates() {
        let Some(gpu) = test_matcher() else { return };
        for seed in 1..=8 {
            let mut rng = Rng::new(seed);
            // Sizes deliberately not multiples of the 16×16 workgroup
            let (w, h) = (40 + rng.below(60), 30 + rng.below(40));
            let screen = random_image(&mut rng, w, h);
            let (tw, th) = (3 + rng.below(12), 3 + rng.below(12));
            let (tx, ty) = (rng.below(w - tw + 1), rng.below(h - th + 1));
            let raw = crop(&screen, tx, ty, tw, th);

            let got = assert_same_as_cpu(&gpu, &screen, &raw);
            assert_eq!((got.x, got.y), (tx, ty), "seed {seed}");
            assert!(got.score > 0.999_999);
        }
    }

    #[test]
    fn gpu_agrees_with_cpu_without_exact_match() {
        let Some(gpu) = test_matcher() else { return };
        for seed in 20..24 {
            let mut rng = Rng::new(seed);
            let screen = smooth_image(&mut rng, 70, 45);
            let other = smooth_image(&mut Rng::new(seed + 50), 70, 45);
            let raw = crop(&other, 11, 7, 9, 8);
            assert_same_as_cpu(&gpu, &screen, &raw);
        }
    }

    #[test]
    fn gpu_flat_screen_scores_zero() {
        let Some(gpu) = test_matcher() else { return };
        let screen = solid_image(50, 40, [200, 10, 90, 255]);
        let raw = random_image(&mut Rng::new(4), 6, 6);
        let got = assert_same_as_cpu(&gpu, &screen, &raw);
        assert_eq!(got.score, 0.0);
    }

    #[test]
    fn gpu_ties_resolve_to_first_position() {
        let Some(gpu) = test_matcher() else { return };
        let raw = random_image(&mut Rng::new(9), 5, 5);
        let mut screen = solid_image(64, 48, [0, 0, 0, 255]);
        // Same row across workgroups, then an earlier row in another group
        paste(&mut screen, &raw, 40, 30);
        paste(&mut screen, &raw, 3, 30);
        paste(&mut screen, &raw, 50, 17);
        let got = assert_same_as_cpu(&gpu, &screen, &raw);
        assert_eq!((got.x, got.y), (50, 17));
    }

    #[test]
    fn gpu_chunked_dispatch_matches_single_dispatch() {
        let Some(mut gpu) = test_matcher() else { return };
        let mut rng = Rng::new(33);
        let screen = random_image(&mut rng, 57, 83);
        let raw = crop(&screen, 21, 64, 7, 5);

        // A tiny budget forces one row per submission (79 submissions)
        gpu.chunk_budget = 1;
        let got = assert_same_as_cpu(&gpu, &screen, &raw);
        assert_eq!((got.x, got.y), (21, 64));
    }

    #[test]
    fn gpu_handles_padded_rows() {
        let Some(gpu) = test_matcher() else { return };
        let mut rng = Rng::new(44);
        let tight = random_image(&mut rng, 33, 21);
        // Same pixels with 12 bytes of padding per row
        let stride = tight.stride + 12;
        let mut data = vec![0xAB; (stride * tight.height) as usize];
        for y in 0..tight.height as usize {
            let src = &tight.data[y * tight.stride as usize..][..tight.stride as usize];
            data[y * stride as usize..][..tight.stride as usize].copy_from_slice(src);
        }
        let padded = ScreenBuffer { data, width: tight.width, height: tight.height, stride };

        let raw = crop(&tight, 17, 9, 6, 6);
        let got = assert_same_as_cpu(&gpu, &padded, &raw);
        assert_eq!((got.x, got.y), (17, 9));
    }

    #[test]
    fn chunk_plan_covers_every_row_once() {
        for (w, h, n, budget) in [(100, 37, 25, 1), (100, 37, 25, 20_000), (5, 300, 9, 1 << 30)] {
            let chunks = plan_chunks(w, h, n, budget, 65_535);
            let mut next = 0;
            let mut slot = 0;
            for c in &chunks {
                assert_eq!(c.row_offset, next);
                assert!(c.row_end > c.row_offset);
                assert_eq!(c.result_offset, slot);
                assert_eq!(c.groups_y, (c.row_end - c.row_offset).div_ceil(WG_EDGE));
                slot += w.div_ceil(WG_EDGE) * c.groups_y;
                next = c.row_end;
            }
            assert_eq!(next, h);
        }
    }

    /// `SYNAPSE_REQUIRE_GPU=1 cargo test --release --lib -- --ignored --nocapture bench_gpu`
    #[test]
    #[ignore]
    fn bench_gpu_vs_cpu() {
        let Some(gpu) = test_matcher() else { return };
        let mut rng = Rng::new(123);
        let screen = smooth_image(&mut rng, 1920, 1080);
        let raw = crop(&screen, 1500, 700, 64, 64);
        let tmpl = PreparedTemplate::new(&raw).unwrap();

        gpu.find_best(&screen, &tmpl).unwrap(); // warm-up (pipeline, allocations)
        let t = std::time::Instant::now();
        let on_gpu = gpu.find_best(&screen, &tmpl).unwrap().unwrap();
        let gpu_time = t.elapsed();

        let t = std::time::Instant::now();
        let on_cpu = ncc::find_best_cpu(&screen, &tmpl).unwrap();
        let cpu_time = t.elapsed();

        assert_eq!((on_gpu.x, on_gpu.y), (on_cpu.x, on_cpu.y));
        println!(
            "1920x1080 / 64x64: GPU [{}] {gpu_time:?}, CPU ({} threads) {cpu_time:?}",
            gpu.adapter_name(),
            rayon::current_num_threads()
        );
    }
}
