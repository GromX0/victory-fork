//! Not a test of the translator: what the host's compiler makes of DXIL.
//! `VICTORY_PROBE=<folder>` and `--ignored probe`: each `.dxil` there (a
//! compute program with a constant buffer `b0` of four dwords and a raw
//! buffer, slot 0 of the view heap) is run on the card. `sem*` programs are
//! given the bits of 0.0, infinity, -0.0 and a NaN and what they store is
//! printed; of the others the time is (several runs of 4096 groups).

use super::HostShader;
use super::host::{Dispatch, Host, Root, View};
use crate::native::{HwStage, RootType};

#[test]
#[ignore = "a tool: VICTORY_PROBE=<folder>"]
fn probe() {
    let Some(folder) = std::env::var_os("VICTORY_PROBE") else { return };
    let host = Host::new(false).expect("a card");
    let mut files: Vec<_> = std::fs::read_dir(folder)
        .expect("the folder")
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "dxil"))
        .collect();
    files.sort();
    for file in files {
        let name = file.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let shader = HostShader {
            stage: HwStage::Cs,
            dxil: std::fs::read(&file).expect("the file"),
            user_data: (0..4)
                .map(|n| super::UserData { index: n, slot: 0, root: RootType::Constants, dword: n })
                .collect(),
            samplers: Vec::new(),
            static_samplers: Vec::new(),
            root_cbvs: Vec::new(),
            root_srvs: Vec::new(),
            root_uavs: Vec::new(),
            root_indexed_views: Vec::new(),
        };
        let (root, pso) = match host.compute_pipeline(&shader) {
            Ok(made) => made,
            Err(e) => {
                println!("{name}: {e}");
                continue;
            }
        };
        let run = |constants: [u32; 4], groups: u32| {
            let began = std::time::Instant::now();
            let out = host
                .dispatch(
                    &root,
                    &pso,
                    &Dispatch {
                        roots: vec![(0, Root::Constants(constants.to_vec()))],
                        views: &[View::RawUav(64)],
                        samplers: &[],
                        groups: [groups, 1, 1],
                    },
                )
                .expect("a dispatch");
            (out[0].clone(), began.elapsed().as_secs_f64() * 1e3)
        };
        if name.starts_with("sem") {
            let (out, _) = run([0, 0x7F80_0000, 0x8000_0000, 0x7FC0_0000], 1);
            for (n, x) in out.iter().enumerate().take(40) {
                println!("{name} {n:>2}: {x:08x} {}", f32::from_bits(*x));
            }
            continue;
        }
        let constants = [0.37f32.to_bits(), 0.11f32.to_bits(), 16384, 0];
        run(constants, 64);
        let mut times: Vec<f64> = (0..9).map(|_| run(constants, 4096).1).collect();
        times.sort_by(f64::total_cmp);
        println!("{name}: {:.2} ms (least {:.2})", times[times.len() / 2], times[0]);
    }
}

/// `VICTORY_PATCH_TEST=<container.cdxil>;<patch file>` and `--ignored
/// patched_translates`: the container translates with the patch file's lines
/// for its hash applied (`VICTORY_CODE_PATCH`'s format, code may grow): for
/// checking a recorder's detour before a run.
#[test]
#[ignore = "a tool: VICTORY_PATCH_TEST=<cdxil>;<patch>"]
fn patched_translates() {
    let Ok(spec) = std::env::var("VICTORY_PATCH_TEST") else { return };
    let (file, patch_file) = spec.split_once(';').expect("<cdxil>;<patch>");
    let bytes = std::fs::read(file).expect("the container");
    let name = std::path::Path::new(file).file_name().unwrap().to_string_lossy().to_ascii_lowercase();
    let hex = |s: &str| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok();
    let mut patch = Vec::new();
    for line in std::fs::read_to_string(patch_file).expect("the patch").lines() {
        let mut words = line.split('#').next().unwrap_or("").split_whitespace();
        let (Some(h), Some(at)) = (words.next(), words.next().and_then(hex)) else { continue };
        if !name.starts_with(h.to_ascii_lowercase().as_str()) {
            continue;
        }
        for (i, w) in words.filter_map(hex).enumerate() {
            patch.push((at as usize + 4 * i, w as u32));
        }
    }
    let plain = super::translate(&bytes).map(|t| t.dxil.len());
    let patched = super::with_code_patch(patch.clone(), || super::translate(&bytes)).map(|t| t.dxil.len());
    println!("{} words patched; plain {plain:?}; patched {patched:?}", patch.len());
    assert!(patched.is_ok(), "{patched:?}");
}

/// `VICTORY_DECODE_FILE=<raw GFX10 code>[;wave32]`: the listing of a bare code blob (a deserialized
/// pipeline's stage, 28 Nov), for reading its exports and interpolants.
#[test]
#[ignore = "diagnostic: needs VICTORY_DECODE_FILE"]
fn decode_raw_code() {
    let Ok(spec) = std::env::var("VICTORY_DECODE_FILE") else { return };
    let (file, wave32) = match spec.split_once(';') {
        Some((f, w)) => (f.to_owned(), w == "wave32"),
        None => (spec.clone(), false),
    };
    let code = std::fs::read(file).expect("the code");
    match crate::gfx10::decode(&code) {
        Ok(insts) => {
            for i in &insts {
                println!("{:06x}: {}", i.offset, i.text(!wave32));
            }
        }
        Err(e) => println!("decode failed: {e}"),
    }
}

/// `VICTORY_DESER_DIR=<dir>`: every dumped serialized mesh pipeline there
/// (`1c8-<n>-<part>.bin`: 0 packet, 2/3 pixel code and metadata, 6/7 the
/// GS stage's) rebuilt as containers ([`crate::deserialized`]) and
/// translated: what fails, and why.
#[test]
#[ignore = "diagnostic: needs VICTORY_DESER_DIR"]
fn deserialized_mesh_pipelines_translate() {
    let Ok(dir) = std::env::var("VICTORY_DESER_DIR") else { return };
    let dir = std::path::PathBuf::from(dir);
    let read = |n: &str, k: u32| std::fs::read(dir.join(format!("1c8-{n}-{k}.bin"))).ok();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("the dump directory")
        .filter_map(|e| {
            let f = e.ok()?.file_name().into_string().ok()?;
            Some(f.strip_prefix("1c8-")?.strip_suffix("-0.bin")?.to_owned())
        })
        .collect();
    names.sort_by_key(|n| n.parse::<u32>().unwrap_or(0));
    let (mut built, mut ok, mut why) = (0, 0, std::collections::BTreeMap::<String, u32>::new());
    for n in &names {
        let (Some(packet), Some(gs), Some(gs_meta)) = (read(n, 0), read(n, 6), read(n, 7)) else { continue };
        let ps = read(n, 2).zip(read(n, 3));
        let parts = crate::deserialized::StageParts { code: &gs, meta: &gs_meta };
        let ps_parts = ps.as_ref().map(|(c, m)| crate::deserialized::StageParts { code: c, meta: m });
        // The world root signature, as the title's own mesh containers lay it
        // out (MSNC_VS_PropFoliageSkinDeferred): records that disagree fail.
        let root = crate::deserialized::RootLayout { sizes: vec![1, 1, 1, 2, 2], resource_heap: true, sampler_heap: true };
        let g = match crate::deserialized::mesh_pipeline(&packet, &root, parts, ps_parts) {
            Ok(g) => g,
            Err(e) => {
                println!("{n}: not rebuilt: {e}");
                *why.entry(format!("rebuild: {e}")).or_default() += 1;
                continue;
            }
        };
        built += 1;
        let mut fine = true;
        for (stage, c) in [("mesh", Some(&g.mesh)), ("pixel", g.pixel.as_ref())] {
            let Some(c) = c else { continue };
            if let Ok(out) = std::env::var("VICTORY_DESER_OUT") {
                let _ = std::fs::write(std::path::Path::new(&out).join(format!("{n}-{stage}.cdxil")), c);
            }
            match crate::emit::translate(c) {
                Ok(_) => {}
                Err(e) => {
                    fine = false;
                    println!("{n} {stage}: {e}");
                    *why.entry(format!("{stage}: {e}")).or_default() += 1;
                }
            }
        }
        ok += u32::from(fine);
    }
    println!("{built} rebuilt, {ok} translated whole");
    for (w, k) in &why {
        println!("{k:5} {w}");
    }
}

/// `VICTORY_CDXIL_DIR=<dir>`: every container there (`VICTORY_FAILED_DUMP`'s
/// `<hash>-<stage>.cdxil`) through `translate`, as a run translates it: why
/// each one fails, counted.
#[test]
#[ignore = "diagnostic: needs VICTORY_CDXIL_DIR"]
fn dumped_containers_translate() {
    let Ok(dir) = std::env::var("VICTORY_CDXIL_DIR") else { return };
    let mut why = std::collections::BTreeMap::<String, Vec<String>>::new();
    let (mut n, mut ok) = (0, 0);
    for e in std::fs::read_dir(dir).expect("the directory").flatten() {
        let Ok(c) = std::fs::read(e.path()) else { continue };
        n += 1;
        match crate::emit::translate(&c) {
            Ok(_) => ok += 1,
            Err(err) => {
                let k = err.to_string();
                let k = k.split_once(": ").map_or(k.clone(), |(_, r)| r.to_owned());
                why.entry(k).or_default().push(e.file_name().to_string_lossy().into_owned());
            }
        }
    }
    println!("{ok} of {n} translate");
    let mut v: Vec<_> = why.into_iter().collect();
    v.sort_by_key(|(_, f)| std::cmp::Reverse(f.len()));
    for (k, f) in v {
        println!("{:4} {k}  (e.g. {})", f.len(), f[0]);
    }
}
