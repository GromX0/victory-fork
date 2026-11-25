//! `victory warm`: the pipelines earlier runs used, made into the host
//! pipeline library before a run, so that the run finds them there instead
//! of making them while the title waits.
//!
//! A run keeps beside the library what each pipeline it drew with or
//! started early is made from (`pipeline-descriptions.txt`: its shaders'
//! content hashes and its graphics state, in the title's own formats). The
//! warm-up finds those shader containers in the title's files, translates
//! them into the shader cache, and makes each pipeline through the code a
//! run makes it with (`victory_gpu::pipeline::Described`), so under the
//! name the run looks it up by. A pipeline already in the library is only
//! loaded. The library is saved as it grows (the library's own saving
//! thread) and at the end, so an interrupted warm-up keeps its work.
//!
//! When to run it: after building a version whose translation changed
//! (the first run of such a build made 1,003 to 2,247 pipelines again on
//! 25 November, minutes of dropped draws), with the switches the runs will
//! have (`VICTORY_SHADER_OPT`, `VICTORY_NAN_GUARD`, `VICTORY_SKIP_PIPELINES`
//! and the rest: a pipeline is named after its translation), and never
//! while the title runs: both would write the library. A second warm-up of
//! the same build makes nothing and takes seconds.
//!
//! `VICTORY_WARM_STALL_SECONDS` (30): a pipeline the host has compiled for
//! that long is given up on: the warm-up saves what it made, lists the
//! pipeline in `warm-stalled.txt` beside the library (later warm-ups leave
//! it out; delete the file to try again) and stops. On AMD's drivers the
//! compile of compute shader 39f0e9aeb156a61c never ends and the driver's
//! memory grows by some 240 MB a second meanwhile (see
//! `VICTORY_SKIP_PIPELINES`).

use std::path::PathBuf;

use anyhow::Result;

/// Runs the warm-up on `jobs` threads (default: as many as a run's
/// pipeline workers) over the title's files under `paths` (default: the
/// configured game).
pub fn run(paths: Vec<PathBuf>, jobs: Option<usize>) -> Result<()> {
    #[cfg(windows)]
    {
        windows_only::run(paths, jobs)
    }
    #[cfg(not(windows))]
    {
        let _ = (paths, jobs);
        anyhow::bail!("the warm-up makes host D3D12 pipelines: Windows only")
    }
}

#[cfg(windows)]
mod windows_only {
    use std::collections::{BTreeMap, HashMap, HashSet};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use anyhow::Result;
    use victory_hle::modules::d3d12x::{self as gpu, Described, Warmed};

    use crate::shader::{MAX_FILE, sources};

    /// Pipelines a stall stopped an earlier warm-up on, by key, beside the
    /// library.
    const STALLED_FILE: &str = "warm-stalled.txt";

    /// Shader containers by their whole content hash.
    type Containers = HashMap<[u8; 32], Vec<u8>>;

    pub(super) fn run(paths: Vec<PathBuf>, jobs: Option<usize>) -> Result<()> {
        let started = Instant::now();
        let cfg = victory_core::Config::load()?;
        // The shader cache and, beside it, the pipeline library, as a run
        // keeps them.
        gpu::use_shader_cache(cfg.cache_subdir("shaders")?);
        let dir = cfg.cache_subdir("pipelines")?;
        let described = gpu::described_pipelines();
        println!(
            "warm-up      {} pipelines described by earlier runs, {} listed to start early ({})",
            described.len(),
            gpu::early_listed(),
            dir.display()
        );
        if described.is_empty() {
            println!(
                "warm-up      nothing to make: a run describes the pipelines it uses beside the library \
                 (pipeline-descriptions.txt), from the first run of a build that keeps descriptions on"
            );
            return Ok(());
        }
        let stalled_before: HashSet<String> = std::fs::read_to_string(dir.join(STALLED_FILE))
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty())
            .collect();
        let (mut todo, mut left_out) = (Vec::new(), BTreeMap::<String, usize>::new());
        for d in described {
            if let Some(hash) = d.skipped() {
                *left_out
                    .entry(format!("skipped: VICTORY_SKIP_PIPELINES names {hash}"))
                    .or_default() += 1;
            } else if stalled_before.contains(d.key()) {
                *left_out
                    .entry(format!(
                        "stalled an earlier warm-up ({STALLED_FILE}): {}",
                        d.label()
                    ))
                    .or_default() += 1;
            } else {
                todo.push(d);
            }
        }
        let threads = jobs.unwrap_or_else(default_threads).max(1);
        let wanted: HashSet<[u8; 32]> = todo.iter().flat_map(|d| d.shaders().copied()).collect();
        let paths = crate::default_paths(paths)?;
        let looked = Instant::now();
        let (containers, files) = find(&paths, &wanted, threads);
        println!(
            "shaders      {} of the {} containers found in {files} files, {:.1} s",
            containers.len(),
            wanted.len(),
            looked.elapsed().as_secs_f64()
        );
        let outcome = make(todo, containers, threads);
        let saved = gpu::save_pipeline_cache_within(Duration::from_secs(60));
        let o = &outcome;
        println!(
            "pipelines    {} made, {} in the library already, {} refused by the host or translation, {:.1} s on {threads} \
             threads{}",
            o.made,
            o.loaded,
            o.refused,
            started.elapsed().as_secs_f64(),
            if saved {
                "; library saved"
            } else {
                "; THE LIBRARY WAS NOT SAVED"
            }
        );
        for (why, n) in o.failed.iter().chain(&left_out) {
            println!("  {n:>7}  {why}");
        }
        if !o.stalled.is_empty() {
            let mut keys: Vec<String> = stalled_before.into_iter().collect();
            keys.extend(o.stalled.iter().map(|(key, _)| key.clone()));
            keys.sort_unstable();
            keys.dedup();
            std::fs::write(dir.join(STALLED_FILE), keys.join("\n") + "\n")?;
            for (_, label) in &o.stalled {
                println!("  stalled  {label}: listed in {STALLED_FILE}, left out from now on");
            }
            anyhow::bail!(
                "the warm-up stopped at {} pipelines the host did not finish compiling \
                 (VICTORY_WARM_STALL_SECONDS); run it again for the rest",
                o.stalled.len()
            );
        }
        anyhow::ensure!(saved, "the pipeline library was not saved");
        Ok(())
    }

    /// As many threads as a run's pipeline workers: half the logical
    /// processors, 2 to 8 (each compile holds driver memory).
    fn default_threads() -> usize {
        std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(2, 8))
    }

    /// Which files are read first: those named after shaders, then
    /// programs and libraries, then the rest. The title's shaders lie in its
    /// executable, `shaders.rpf` and two libraries; read in the folder's
    /// order, every one was found only after 86 files and 33 s.
    fn read_first(path: &std::path::Path) -> u8 {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if name.contains("shader") {
            0
        } else if name.ends_with(".exe") || name.ends_with(".dll") {
            1
        } else {
            2
        }
    }

    /// The containers of `wanted` in the files under `paths`, read on
    /// `threads` threads, and how many files were read. Stops once every one
    /// is found.
    fn find(paths: &[PathBuf], wanted: &HashSet<[u8; 32]>, threads: usize) -> (Containers, usize) {
        let found: Mutex<Containers> = Mutex::new(HashMap::new());
        let mut listed: Vec<PathBuf> = (paths.iter())
            .flat_map(|root| {
                walkdir::WalkDir::new(root)
                    .into_iter()
                    .filter_map(Result::ok)
            })
            .filter(|e| e.file_type().is_file() && e.metadata().is_ok_and(|m| m.len() <= MAX_FILE))
            .map(|e| e.into_path())
            .collect();
        listed.sort_by_key(|p| read_first(p));
        let mut files = 0;
        for path in &listed {
            if lock(&found).len() == wanted.len() {
                break;
            }
            let Ok(data) = std::fs::read(path) else {
                continue;
            };
            files += 1;
            let list = sources(&data);
            let next = AtomicUsize::new(0);
            std::thread::scope(|s| {
                for _ in 0..threads.min(list.len()) {
                    s.spawn(|| {
                        while let Some(source) = list.get(next.fetch_add(1, Ordering::Relaxed)) {
                            let Some(bytes) = source.bytes() else {
                                continue;
                            };
                            let hash = *blake3::hash(&bytes).as_bytes();
                            if wanted.contains(&hash) {
                                lock(&found)
                                    .entry(hash)
                                    .or_insert_with(|| bytes.into_owned());
                            }
                        }
                    });
                }
            });
        }
        (found.into_inner().unwrap_or_else(|e| e.into_inner()), files)
    }

    /// What the making came to.
    #[derive(Default)]
    struct Outcome {
        made: usize,
        loaded: usize,
        refused: usize,
        /// Pipelines not tried, by why.
        failed: BTreeMap<String, usize>,
        /// Key and shaders of each pipeline given up on.
        stalled: Vec<(String, String)>,
    }

    /// Milliseconds since `since`, never 0 (0 marks an idle worker).
    fn ms(since: Instant) -> u64 {
        since.elapsed().as_millis() as u64 + 1
    }

    /// Makes `todo` on `threads` threads of their own, reporting progress,
    /// until all are done or one stalls ([`super`]'s notes).
    fn make(todo: Vec<Described>, containers: Containers, threads: usize) -> Outcome {
        let stall = Duration::from_secs(
            std::env::var("VICTORY_WARM_STALL_SECONDS")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(30),
        );
        let n = todo.len();
        let todo = Arc::new(todo);
        let containers = Arc::new(containers);
        let next = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let clock = Instant::now();
        // Per worker: when its pipeline began (`ms`; 0: none), which one,
        // and whether it has finished.
        let slots: Arc<Vec<[AtomicU64; 3]>> = Arc::new(
            (0..threads)
                .map(|_| [const { AtomicU64::new(0) }; 3])
                .collect(),
        );
        // Made, loaded, refused; and why others were not tried.
        let counts = Arc::new([const { AtomicUsize::new(0) }; 3]);
        let failed: Arc<Mutex<BTreeMap<String, usize>>> = Arc::default();
        for (w, _) in slots.iter().enumerate() {
            let (todo, containers, next, stop, own_slots, counts, failed) = (
                todo.clone(),
                containers.clone(),
                next.clone(),
                stop.clone(),
                slots.clone(),
                counts.clone(),
                failed.clone(),
            );
            let worker = move || {
                let slot = &own_slots[w];
                while !stop.load(Ordering::Relaxed) {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(d) = todo.get(i) else { break };
                    slot[1].store(i as u64, Ordering::Relaxed);
                    slot[0].store(ms(clock), Ordering::Relaxed);
                    let began = Instant::now();
                    let warmed = d.warm(|hash| containers.get(hash).map(Vec::as_slice));
                    slot[0].store(0, Ordering::Relaxed);
                    let counted = match warmed {
                        Ok(Warmed::Made) => 0,
                        Ok(Warmed::Loaded) => 1,
                        Ok(Warmed::Refused) => 2,
                        Err(why) => {
                            *lock(&failed).entry(why.to_owned()).or_default() += 1;
                            3
                        }
                    };
                    if let Some(count) = counts.get(counted) {
                        count.fetch_add(1, Ordering::Relaxed);
                    }
                    if began.elapsed() > Duration::from_secs(10) {
                        println!(
                            "  slow     {} took {:.1} s",
                            d.label(),
                            began.elapsed().as_secs_f64()
                        );
                    }
                }
                slot[2].store(1, Ordering::Relaxed);
            };
            if let Err(e) = std::thread::Builder::new()
                .name(format!("victory-warm-{w}"))
                .spawn(worker)
            {
                eprintln!("warm-up      no worker thread: {e}");
                slots[w][2].store(1, Ordering::Relaxed);
            }
        }
        let mut stalled: Vec<usize> = Vec::new();
        let mut last_report = Instant::now();
        let mut stopped_at: Option<Instant> = None;
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let now = ms(clock);
            for slot in slots.iter() {
                let began = slot[0].load(Ordering::Relaxed);
                let i = slot[1].load(Ordering::Relaxed) as usize;
                if began != 0
                    && now.saturating_sub(began) > stall.as_millis() as u64
                    && !stalled.contains(&i)
                {
                    stalled.push(i);
                    println!(
                        "  stalled  {} still compiling after {} s: no more pipelines are started",
                        todo[i].label(),
                        stall.as_secs()
                    );
                    stop.store(true, Ordering::Relaxed);
                    stopped_at.get_or_insert_with(Instant::now);
                }
            }
            // Workers not finished, and of them those stuck in a stalled
            // compile (which never finish).
            let live = slots
                .iter()
                .filter(|s| s[2].load(Ordering::Relaxed) == 0)
                .count();
            let stuck = (slots.iter())
                .filter(|s| s[2].load(Ordering::Relaxed) == 0 && s[0].load(Ordering::Relaxed) != 0)
                .filter(|s| stalled.contains(&(s[1].load(Ordering::Relaxed) as usize)))
                .count();
            // After a stall the others finish what they compile, a little while.
            let waited_enough = stopped_at.is_some_and(|t| t.elapsed() > Duration::from_secs(10));
            if live == stuck || waited_enough {
                break;
            }
            if last_report.elapsed() >= Duration::from_secs(10) {
                last_report = Instant::now();
                let [made, loaded, refused] = [0, 1, 2].map(|i| counts[i].load(Ordering::Relaxed));
                println!(
                    "warm-up      {} of {n} pipelines: {made} made, {loaded} in the library already, {refused} refused; {:.0} s",
                    made + loaded + refused + lock(&failed).values().sum::<usize>(),
                    clock.elapsed().as_secs_f64()
                );
            }
        }
        let [made, loaded, refused] = [0, 1, 2].map(|i| counts[i].load(Ordering::Relaxed));
        let mut failed = lock(&failed).clone();
        let untried = n.saturating_sub(next.load(Ordering::Relaxed).min(n));
        if untried > 0 {
            failed.insert(
                "not tried: the warm-up stopped at a stall".to_owned(),
                untried,
            );
        }
        Outcome {
            made,
            loaded,
            refused,
            failed,
            stalled: stalled
                .iter()
                .map(|&i| (todo[i].key().to_owned(), todo[i].label()))
                .collect(),
        }
    }

    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|e| e.into_inner())
    }
}
