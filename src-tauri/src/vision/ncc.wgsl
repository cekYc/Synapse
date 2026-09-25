// ============================================================
// Synapse — NCC template matching compute shader
// ============================================================
// One invocation scores one candidate position (sx, sy) with
// zero-mean normalized cross-correlation (RGB, alpha ignored).
// Each 16×16 workgroup then reduces its scores in shared memory
// and writes a single (score, index) pair, so the host only reads
// back one candidate per workgroup instead of the full score map.
//
// Values are normalized to [0, 1] and the patch mean is computed in
// a first pass so the variance never suffers from the cancellation
// of the Σx² − (Σx)²/n form in f32.
// ============================================================

struct Params {
    screen_w: u32,      // screen width in pixels (tightly packed rows)
    search_w: u32,      // number of candidate columns
    row_offset: u32,    // first candidate row of this dispatch
    row_end: u32,       // one past the last candidate row of this dispatch
    tmpl_w: u32,
    tmpl_h: u32,
    result_offset: u32, // first result slot of this dispatch
    _pad0: u32,
    inv_n: f32,         // 1 / (tmpl_w * tmpl_h)
    tmpl_norm: f32,     // ‖T − μT‖ in normalized units
    _pad1: f32,
    _pad2: f32,
}

@group(0) @binding(0) var<storage, read> screen: array<u32>;         // packed BGRA8
@group(0) @binding(1) var<storage, read> tmpl: array<vec4<f32>>;     // T − μT (rgb)
@group(0) @binding(2) var<storage, read_write> results: array<vec2<u32>>;
@group(0) @binding(3) var<uniform> params: Params;

const WG_SIZE: u32 = 256u;
const INVALID: u32 = 0xffffffffu;

var<workgroup> wg_score: array<f32, WG_SIZE>;
var<workgroup> wg_index: array<u32, WG_SIZE>;

fn rgb(p: u32) -> vec3<f32> {
    // Little-endian BGRA: unpack yields (B, G, R, A)
    return unpack4x8unorm(p).zyx;
}

fn ncc_at(sx: u32, sy: u32) -> f32 {
    // Pass 1: patch mean
    var sum = vec3<f32>(0.0);
    for (var ty = 0u; ty < params.tmpl_h; ty++) {
        let row = (sy + ty) * params.screen_w + sx;
        for (var tx = 0u; tx < params.tmpl_w; tx++) {
            sum += rgb(screen[row + tx]);
        }
    }
    let mean = sum * params.inv_n;

    // Pass 2: centered correlation and patch variance
    var xcorr = 0.0;
    var var_sum = 0.0;
    var t = 0u;
    for (var ty = 0u; ty < params.tmpl_h; ty++) {
        let row = (sy + ty) * params.screen_w + sx;
        for (var tx = 0u; tx < params.tmpl_w; tx++) {
            let d = rgb(screen[row + tx]) - mean;
            xcorr += dot(d, tmpl[t].xyz);
            var_sum += dot(d, d);
            t++;
        }
    }

    // Flat patches have no defined correlation; the CPU path scores them 0
    let denom = params.tmpl_norm * sqrt(var_sum);
    if (var_sum < 1e-9 || denom < 1e-12) {
        return 0.0;
    }
    return clamp(xcorr / denom, 0.0, 1.0);
}

@compute @workgroup_size(16, 16, 1)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
) {
    let sx = gid.x;
    let sy = params.row_offset + gid.y;

    var score = -1.0;
    var index = INVALID;
    if (sx < params.search_w && sy < params.row_end) {
        score = ncc_at(sx, sy);
        index = sy * params.search_w + sx;
    }
    wg_score[lid] = score;
    wg_index[lid] = index;
    workgroupBarrier();

    // Tree reduction: highest score wins, ties go to the lowest index
    // (row-major), matching the CPU matcher's tie-breaking.
    for (var stride = WG_SIZE / 2u; stride > 0u; stride = stride / 2u) {
        if (lid < stride) {
            let other_score = wg_score[lid + stride];
            let other_index = wg_index[lid + stride];
            let mine = wg_score[lid];
            if (other_score > mine || (other_score == mine && other_index < wg_index[lid])) {
                wg_score[lid] = other_score;
                wg_index[lid] = other_index;
            }
        }
        workgroupBarrier();
    }

    if (lid == 0u) {
        let slot = params.result_offset + wid.y * nwg.x + wid.x;
        results[slot] = vec2<u32>(bitcast<u32>(wg_score[0]), wg_index[0]);
    }
}
