//! Colour targets of the formats the title renders to, drawn on the host GPU:
//! what the card stores for bright colours, against what the console's colour
//! blocks store for them (the D3D conversion rules).

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use super::HostShader;
use super::bitcode::{Attr, Term};
use super::container::SigElement;
use super::host::{Draw, Host};
use super::shader::Shader;
use crate::native::HwStage;

fn element(name: &str, kind: u8, row: u32) -> SigElement {
    SigElement {
        name: name.into(),
        index: 0,
        kind,
        comp: 9,
        interp: 0,
        row: Some(row),
        col: 0,
        cols: 4,
        usage: 15,
        half: false,
    }
}

fn shader(stage: HwStage, dxil: Vec<u8>) -> HostShader {
    HostShader {
        stage,
        dxil,
        user_data: Vec::new(),
        samplers: Vec::new(),
        static_samplers: Vec::new(),
        root_cbvs: Vec::new(),
        root_srvs: Vec::new(),
        root_uavs: Vec::new(),
        root_indexed_views: Vec::new(),
    }
}

/// A vertex shader passing POSITION through to SV_Position.
fn position_vs() -> HostShader {
    let mut s = Shader::new(false);
    s.inputs = vec![element("POSITION", 0, 0)];
    s.outputs = vec![SigElement {
        interp: 4,
        ..element("SV_Position", 3, 0)
    }];
    for col in 0..4 {
        let args = [s.m.i32(0), s.m.i32(0), s.m.i8(col), s.m.undef(s.t.i32)];
        let v = s.op(s.entry, "loadInput.f32", Attr::ReadNone, s.t.f32, 4, &args);
        let args = [s.m.i32(0), s.m.i32(0), s.m.i8(col), v];
        s.op(s.entry, "storeOutput.f32", Attr::NoUnwind, s.t.void, 5, &args);
    }
    s.m.terminate(s.entry, Term::Ret);
    shader(HwStage::Vs, s.finish())
}

/// A pixel shader writing the float bits `color` to target 0, as the
/// title's colour exports reach the host's output merger.
fn color_ps(color: [u32; 4]) -> HostShader {
    let mut s = Shader::new(true);
    s.outputs.push(element("SV_Target", 16, 0));
    for (c, bits) in color.into_iter().enumerate() {
        let args = [s.m.i32(0), s.m.i32(0), s.m.i8(c as u8), s.m.f32(bits)];
        s.op(s.entry, "storeOutput.f32", Attr::NoUnwind, s.t.void, 5, &args);
    }
    s.m.terminate(s.entry, Term::Ret);
    shader(HwStage::Ps, s.finish())
}

/// What R9G9B9E5_SHAREDEXP holds for `rgb` by the D3D conversion rules (the
/// console's colour block's): each channel clamped to [0, 65408] (NaN 0),
/// the exponent shared from the largest, each mantissa rounded to nearest.
fn rgb9e5(rgb: [f32; 3]) -> u32 {
    const N: i32 = 9;
    const B: i32 = 15;
    let max = 511.0 / 512.0 * 65536.0;
    let [r, g, b] = rgb.map(|x| {
        let x = f64::from(x);
        if x.is_nan() || x <= 0.0 { 0.0 } else { x.min(max) }
    });
    let most = r.max(g).max(b);
    let floor_log = if most > 0.0 { most.log2().floor() as i32 } else { -B - 1 };
    let mut e = (-B - 1).max(floor_log) + 1 + B;
    let step = |e: i32| 2f64.powi(e - B - N);
    if (most / step(e) + 0.5).floor() == 512.0 {
        e += 1;
    }
    let m = |x: f64| (x / step(e) + 0.5).floor() as u32;
    m(r) | m(g) << 9 | m(b) << 18 | (e as u32) << 27
}

/// The three channels R9G9B9E5_SHAREDEXP bits hold.
fn from_rgb9e5(x: u32) -> [f64; 3] {
    let step = 2f64.powi((x >> 27) as i32 - 15 - 9);
    [0, 9, 18].map(|at| f64::from(x >> at & 0x1FF) * step)
}

/// One-pixel draws of `ps` into a target of `format` cleared to `clear`
/// (with `add`, the colour is added to it): the texel's bytes, per backend.
fn draw_pixel(
    host: &Host,
    ps: &HostShader,
    format: DXGI_FORMAT,
    texel: usize,
    clear: [f32; 4],
    add: bool,
) -> Result<Vec<u8>, String> {
    let vs = position_vs();
    let root = host.root_signature(&vs, ps)?;
    let pso = host.pipeline_into(&root, &vs.dxil, None, &ps.dxil, Some((format, add)))?;
    let vertices = [
        -1.0, 1.0, 0.5, 1.0, -1.0, -3.0, 0.5, 1.0, 3.0, 1.0, 0.5, 1.0,
    ];
    host.draw_into(
        &root,
        &pso,
        &Draw {
            roots: Vec::new(),
            textures: &[],
            samplers: &[],
            vertices: &vertices,
            stride: 16,
            size: (1, 1),
            mesh_groups: 0,
        },
        format,
        texel,
        clear,
    )
}


/// Whether `host` renders to `format` (the RX 9070 does to R9G9B9E5, so the title's
/// R9G9B9E5 targets are the real format there, where NVIDIA gets the 16-bit float
/// stand-in: `victory-gpu` `resource::target_format`).
fn renders_to(host: &Host, format: DXGI_FORMAT) -> bool {
    let mut support = D3D12_FEATURE_DATA_FORMAT_SUPPORT {
        Format: format,
        ..Default::default()
    };
    // SAFETY: a plain query into our own structure.
    let asked = unsafe {
        host.device.CheckFeatureSupport(
            D3D12_FEATURE_FORMAT_SUPPORT,
            (&raw mut support).cast(),
            size_of_val(&support) as u32,
        )
    };
    asked.is_ok() && support.Support1.0 & D3D12_FORMAT_SUPPORT1_RENDER_TARGET.0 != 0
}

/// `got` (R9G9B9E5 bits) against `rgb`: the exponent the D3D rules share, each mantissa
/// that or the one below (the RX 9070's colour blocks truncate where WARP rounds), no
/// channel lost.
fn check_rgb9e5(what: &str, rgb: [f32; 3], got: u32) {
    let want = rgb9e5(rgb);
    assert_eq!(got >> 27, want >> 27, "{what}: {rgb:?} gave {got:#010x} ({:?}), want {want:#010x}", from_rgb9e5(got));
    for at in [0, 9, 18] {
        let (g, w) = (got >> at & 0x1FF, want >> at & 0x1FF);
        assert!(
            g == w || g + 1 == w,
            "{what}: {rgb:?} gave {got:#010x} ({:?}), want {want:#010x} ({:?}): channel at bit {at}",
            from_rgb9e5(got),
            from_rgb9e5(want),
        );
    }
}

/// Bright colours drawn to an R9G9B9E5 target, and added to one by blending: every channel
/// kept as the D3D rules say (clamped to [0, 65408], NaN 0, the exponent shared from the
/// largest), a green past the format's top included. And a green channel past the top of
/// a 16-bit float target saturates there (65504; infinity at most), red and blue kept.
#[test]
fn bright_colours_keep_every_channel_in_r9g9b9e5_and_f16_targets() {
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let cases: &[[f32; 3]] = &[
        [1.0, 0.5, 0.25],
        [1000.0, 2000.0, 3000.0],
        [60000.0, 65000.0, 50000.0],
        [65408.0, 65408.0, 65408.0],
        [65472.0, 65472.0, 65472.0],
        [65504.0, 65504.0, 65504.0],
        [40000.0, 65504.0, 40000.0],
        [65504.0, 40000.0, 40000.0],
        [40000.0, 40000.0, 65504.0],
        [30000.0, 65480.0, 20000.0],
        [70000.0, 70000.0, 70000.0],
        [50000.0, 70000.0, 50000.0],
        [1e6, 1e6, 1e6],
        [f32::INFINITY, 1.0, 1.0],
        [-1.0, 5.0, 5.0],
        [f32::NAN, 1.0, 1.0],
    ];
    let mut ran = 0;
    for (backend, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else { continue };
        if !renders_to(&host, DXGI_FORMAT_R9G9B9E5_SHAREDEXP) {
            eprintln!("{backend}: no R9G9B9E5 targets; skipped");
            continue;
        }
        let word = |b: Vec<u8>| u32::from_le_bytes(b[..4].try_into().unwrap());
        for &c in cases {
            let ps = color_ps([c[0].to_bits(), c[1].to_bits(), c[2].to_bits(), 1f32.to_bits()]);
            let got = draw_pixel(&host, &ps, DXGI_FORMAT_R9G9B9E5_SHAREDEXP, 4, [0.0; 4], false)
                .map(word)
                .unwrap_or_else(|e| panic!("{backend}: {e}"));
            check_rgb9e5(backend, c, got);
        }
        for (clear, c) in [
            ([40000.0f32, 40000.0, 40000.0], [40000.0f32, 40000.0, 40000.0]),
            ([30000.0, 60000.0, 30000.0], [3000.0, 6000.0, 3000.0]),
            ([1.0, 2.0, 3.0], [1.0, 1.0, 1.0]),
        ] {
            let ps = color_ps([c[0].to_bits(), c[1].to_bits(), c[2].to_bits(), 1f32.to_bits()]);
            let clear4 = [clear[0], clear[1], clear[2], 1.0];
            let got = draw_pixel(&host, &ps, DXGI_FORMAT_R9G9B9E5_SHAREDEXP, 4, clear4, true)
                .map(word)
                .unwrap_or_else(|e| panic!("{backend}: {e}"));
            // (the clear itself is a 9e5 value: the sum of that and the colour)
            let stored = from_rgb9e5(rgb9e5(clear));
            let sum = std::array::from_fn(|k| (stored[k] + f64::from(c[k])) as f32);
            check_rgb9e5(&format!("{backend}, added to {clear:?}"), sum, got);
        }
        let ps = color_ps([40000f32.to_bits(), 70000f32.to_bits(), 40000f32.to_bits(), 1f32.to_bits()]);
        let got = draw_pixel(&host, &ps, DXGI_FORMAT_R16G16B16A16_FLOAT, 8, [0.0; 4], false)
            .unwrap_or_else(|e| panic!("{backend}: {e}"));
        let h = [0, 2, 4].map(|k| u16::from_le_bytes([got[k], got[k + 1]]));
        assert!(
            h[0] == 0x78E2 && h[2] == 0x78E2 && (h[1] == 0x7BFF || h[1] == 0x7C00),
            "{backend}: (40000, 70000, 40000) in RGBA16F: {h:#06x?}"
        );
        ran += 1;
    }
    if ran == 0 {
        eprintln!("no device renders to R9G9B9E5; not run");
    }
}

/// `draw_pixel` into a `size` x `size` target, then the target copied into a texture of
/// `copy_to` (whole, or with `boxed` all but its first row and column): the target's bytes
/// and the copy's.
fn draw_and_copy(
    host: &Host,
    ps: &HostShader,
    format: DXGI_FORMAT,
    texel: usize,
    size: u32,
    copy_to: DXGI_FORMAT,
    boxed: bool,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let vs = position_vs();
    let root = host.root_signature(&vs, ps)?;
    let pso = host.pipeline_into(&root, &vs.dxil, None, &ps.dxil, Some((format, false)))?;
    let vertices = [
        -1.0, 1.0, 0.5, 1.0, -1.0, -3.0, 0.5, 1.0, 3.0, 1.0, 0.5, 1.0,
    ];
    host.draw_and_copy(
        &root,
        &pso,
        &Draw {
            roots: Vec::new(),
            textures: &[],
            samplers: &[],
            vertices: &vertices,
            stride: 16,
            size: (size, size),
            mesh_groups: 0,
        },
        format,
        texel,
        [0.0; 4],
        copy_to,
        boxed,
    )
}

/// The console copies a texture into one of another format of the same texel width as it
/// is, bit for bit (the title's own copies between its R9G9B9E5 and R11G11B10 targets and
/// R32_UINT textures). The host passes such copies to the card as they come (on NVIDIA the
/// R9G9B9E5 side is a stand-in and the copy is skipped). Drawn first, so that the source is
/// a target as the card keeps one (compressed), then copied whole, and a box of it.
#[test]
fn copies_between_formats_of_one_texel_width_keep_the_bits() {
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Ok(host) = Host::new(false) else {
        eprintln!("no SM 6.6 hardware device; not run");
        return;
    };
    let pairs = [
        (DXGI_FORMAT_R9G9B9E5_SHAREDEXP, DXGI_FORMAT_R32_UINT),
        (DXGI_FORMAT_R9G9B9E5_SHAREDEXP, DXGI_FORMAT_R11G11B10_FLOAT),
        (DXGI_FORMAT_R9G9B9E5_SHAREDEXP, DXGI_FORMAT_R8G8B8A8_UNORM),
        (DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R32_UINT),
        (DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R9G9B9E5_SHAREDEXP),
        (DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R9G9B9E5_SHAREDEXP),
        (DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R9G9B9E5_SHAREDEXP),
    ];
    let colours: [[f32; 4]; 2] = [[0.5, 0.25, 1.0, 1.0], [40000.0, 65000.0, 30000.0, 1.0]];
    let mut lost = Vec::new();
    for (from, to) in pairs {
        if !renders_to(&host, from) {
            eprintln!("no {from:?} targets; skipped");
            continue;
        }
        for c in colours {
            let ps = color_ps(c.map(f32::to_bits));
            for (size, boxed) in [(1, false), (128, false), (128, true)] {
                let (target, copy) = match draw_and_copy(&host, &ps, from, 4, size, to, boxed) {
                    Ok(both) => both,
                    // The RX 9070 refuses a box copied between most such pairs (all but R32's
                    // family and R9G9B9E5): its list does not close. `victory-gpu` copies those
                    // through a buffer (`list::copies_through_buffer`).
                    Err(e) if boxed => {
                        eprintln!("{from:?} -> {to:?} boxed: refused ({e})");
                        continue;
                    }
                    Err(e) => panic!("{from:?} to {to:?}: {e}"),
                };
                let word = |b: &[u8]| u32::from_le_bytes(b[..4].try_into().unwrap());
                // (boxed: the texels of the first row and column were not copied)
                let copied = |n: usize| !boxed || (!n.is_multiple_of(size as usize) && n >= size as usize);
                let differ = target
                    .chunks(4)
                    .zip(copy.chunks(4))
                    .enumerate()
                    .filter(|&(n, (a, b))| copied(n) && a != b)
                    .count();
                let last = target.len() - 4;
                eprintln!(
                    "{from:?} -> {to:?} {size}x{size} boxed {boxed} {c:?}: target {:#010x}, copy {:#010x}, {differ} texels differ",
                    word(&target[last..]),
                    word(&copy[last..]),
                );
                if differ != 0 {
                    lost.push(format!("{from:?} -> {to:?} {size}x{size} boxed {boxed} {c:?}: {differ} texels"));
                }
            }
        }
    }
    assert!(lost.is_empty(), "copies that changed bits: {lost:#?}");
}

/// A compute shader storing `color` through a typed UAV at heap slot 0 (texel 0, 0).
fn store_cs(color: [f32; 4]) -> HostShader {
    let mut s = Shader::new(false);
    s.threads = Some([1, 1, 1]);
    let e = s.entry;
    let zero = s.m.i32(0);
    // a 2D texture UAV of four f32 channels (the translator's typed image store)
    let h = s.heap_handle(e, zero, false, false, (2 | 0x1000, 0x409));
    let u = s.m.undef(s.t.i32);
    let mut args = vec![h, zero, zero, u];
    args.extend(color.map(|c| s.m.f32(c.to_bits())));
    args.push(s.m.i8(0xF));
    let void = s.t.void;
    s.op(e, "textureStore.f32", Attr::NoUnwind, void, 67, &args);
    s.m.terminate(e, Term::Ret);
    shader(HwStage::Cs, s.finish())
}

/// Typed stores into an R9G9B9E5 texture (the title's targets of it may also allow
/// unordered access, and the RX 9070 says it takes typed stores of the format) keep every
/// channel as the D3D rules say, also for a texture that is a render target too.
#[test]
fn typed_stores_into_r9g9b9e5_keep_every_channel() {
    use super::host::{Dispatch, Root, View};
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Ok(host) = Host::new(false) else {
        eprintln!("no SM 6.6 hardware device; not run");
        return;
    };
    let mut support = D3D12_FEATURE_DATA_FORMAT_SUPPORT {
        Format: DXGI_FORMAT_R9G9B9E5_SHAREDEXP,
        ..Default::default()
    };
    // SAFETY: a plain query into our own structure.
    let asked = unsafe {
        host.device.CheckFeatureSupport(
            D3D12_FEATURE_FORMAT_SUPPORT,
            (&raw mut support).cast(),
            size_of_val(&support) as u32,
        )
    };
    if asked.is_err() || support.Support2.0 & D3D12_FORMAT_SUPPORT2_UAV_TYPED_STORE.0 == 0 {
        eprintln!("no typed stores of R9G9B9E5 here; not run");
        return;
    }
    let cases: &[[f32; 3]] = &[
        [1.0, 0.5, 0.25],
        [60000.0, 65000.0, 50000.0],
        [40000.0, 70000.0, 40000.0],
        [65504.0, 65504.0, 65504.0],
        [-1.0, 5.0, 5.0],
    ];
    for flags in [D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET] {
        for &c in cases {
            let cs = store_cs([c[0], c[1], c[2], 1.0]);
            if let Ok(mut v) = super::validator::Validator::load(super::validator::Validator::default_path().as_deref()) {
                let mut copy = cs.dxil.clone();
                v.validate(&mut copy).expect("DXIL validation failed");
            }
            let (root, pso) = host.compute_pipeline(&cs).unwrap_or_else(|e| panic!("pipeline: {e}"));
            let out = host
                .dispatch(
                    &root,
                    &pso,
                    &Dispatch {
                        roots: vec![(0, Root::Constants(vec![0]))],
                        views: &[View::FormatTextureUav(1, 1, DXGI_FORMAT_R9G9B9E5_SHAREDEXP, flags)],
                        samplers: &[],
                        groups: [1, 1, 1],
                    },
                )
                .unwrap_or_else(|e| panic!("dispatch: {e}"));
            check_rgb9e5(&format!("typed store, flags {flags:?}"), c, out[0][0]);
        }
    }
}
