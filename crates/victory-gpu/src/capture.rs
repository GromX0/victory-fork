//! Presented frames written to PNG files, to see what reaches the screen:
//! with `VICTORY_CAPTURE=<folder>`, every frame whose number is a power of
//! two (the frames the log reports) is saved as `frame-<n>.png`, at half
//! size. Only in builds with diagnostics.

use std::path::PathBuf;

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// The folder frames go to, if capturing.
pub(crate) fn folder() -> Option<&'static PathBuf> {
    static FOLDER: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| std::env::var_os("VICTORY_CAPTURE").map(PathBuf::from))
        .as_ref()
}

/// Frames listed in `VICTORY_CAPTURE_FRAMES` (comma-separated): saved
/// too, and their draws traced at debug level.
pub(crate) fn listed(n: u64) -> bool {
    static LIST: std::sync::OnceLock<Vec<u64>> = std::sync::OnceLock::new();
    let named = LIST
        .get_or_init(|| {
            let list = std::env::var("VICTORY_CAPTURE_FRAMES").unwrap_or_default();
            list.split(',')
                .filter_map(|f| f.trim().parse().ok())
                .collect()
        })
        .contains(&n)
        || keyed(n);
    // `VICTORY_CAPTURE_WORLD`: also the 2nd and the 6th frame after the first
    // one that ran 500 draws or more (a title's first 3D frames: their
    // numbers depend on how long the menus were shown), and the 30th and
    // the 80th (the title opens with a cutscene made in the
    // engine: later shots of it, in case the first is dark by design).
    //
    // `VICTORY_CAPTURE_WORLD_ALSO` (comma-separated): more frames counted
    // from that first one (the title's blue patches show from
    // about the 118th frame on, when the window's model has arrived and
    // the reflections hold values: none of the four above has them).
    static ALSO: std::sync::OnceLock<Vec<u64>> = std::sync::OnceLock::new();
    let also = ALSO.get_or_init(|| {
        let list = std::env::var("VICTORY_CAPTURE_WORLD_ALSO").unwrap_or_default();
        list.split(',')
            .filter_map(|f| f.trim().parse().ok())
            .collect()
    });
    let first = WORLD.load(std::sync::atomic::Ordering::Relaxed);
    let after = n.wrapping_sub(first);
    named || first != u64::MAX && ([2, 6, 30, 80].contains(&after) || also.contains(&after))
}

/// With a capture folder, for the frames [`listed`]: `what` is added to
/// the frame's `frame-<n>-ops.txt`, one line per draw, dispatch, copy or
/// drop in the order recorded (the title records on several threads, so
/// lines of different lists interleave; each names its list). The file
/// says which pass writes which target, to find the pass whose output
/// stays empty.
#[cfg(feature = "diagnostics")]
pub(crate) fn op(list: usize, what: impl FnOnce() -> String) {
    use std::io::Write;
    let frame = crate::queue::presented() + 1;
    if !listed(frame) {
        // Before the world: the last lines are kept, and written out when
        // it starts (`before-world-ops.txt`): what the title drew just
        // before (the title shows a title card there, whose text
        // did not show).
        if world_on() && WORLD.load(std::sync::atomic::Ordering::Relaxed) == u64::MAX {
            // At most 300 lines of a frame: the world's first frame alone
            // (thousands of operations, recorded before it is known to be
            // the world's) pushed everything before it out.
            static IN_FRAME: std::sync::Mutex<(u64, u32)> = std::sync::Mutex::new((0, 0));
            {
                let mut in_frame = IN_FRAME.lock().unwrap_or_else(|e| e.into_inner());
                if in_frame.0 != frame {
                    *in_frame = (frame, 0);
                }
                in_frame.1 += 1;
                if in_frame.1 > 300 {
                    return;
                }
            }
            let what = what();
            let mut ring = RING.lock().unwrap_or_else(|e| e.into_inner());
            if ring.len() >= RING_LINES {
                ring.pop_front();
            }
            ring.push_back(format!("frame {frame} list {list:#x} {what}"));
        }
        return;
    }
    let Some(folder) = folder() else {
        return;
    };
    static FILE: std::sync::Mutex<Option<(u64, u64, std::fs::File)>> = std::sync::Mutex::new(None);
    let what = what();
    let mut file = FILE.lock().unwrap_or_else(|e| e.into_inner());
    if file.as_ref().is_none_or(|f| f.0 != frame) {
        let Ok(made) = std::fs::File::create(folder.join(format!("frame-{frame}-ops.txt"))) else {
            return;
        };
        *file = Some((frame, 0, made));
    }
    if let Some((_, n, f)) = file.as_mut() {
        *n += 1;
        let _ = writeln!(f, "{n} list {list:#x} {what}");
    }
}

/// The last lines of [`op`] before the world.
#[cfg(feature = "diagnostics")]
static RING: std::sync::Mutex<std::collections::VecDeque<String>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());
#[cfg(feature = "diagnostics")]
const RING_LINES: usize = 6000;

/// Whether `VICTORY_CAPTURE_WORLD` is set.
fn world_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_CAPTURE_WORLD").is_some())
}

/// The first frame that ran 500 draws or more (`VICTORY_CAPTURE_WORLD`).
static WORLD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(u64::MAX);

/// Consecutive frames an F9 press or a timed capture takes
/// (`VICTORY_CAPTURE_SPAN`, 1): the title records a frame while the one
/// before is shown, so the operations attributed to one frame number can be
/// part of two; several in a row hold at least one whole frame's work.
fn span() -> u64 {
    static N: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *N.get_or_init(|| std::env::var("VICTORY_CAPTURE_SPAN").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(1).max(1))
}

/// When the first frame of 500 draws or more was presented, whatever the
/// capture switches: [`world_started`].
static WORLD_AT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// See [`crate::world_started`].
pub(crate) fn world_started() -> Option<std::time::Instant> {
    WORLD_AT.get().copied()
}

/// The frames F9 or a timed capture asked for, the last 64: see [`key_pressed`].
/// (A list: timed captures a second apart while the title presents two
/// frames a second, loading, overwrote each other in one slot.)
static KEYED: std::sync::Mutex<std::collections::VecDeque<u64>> = std::sync::Mutex::new(std::collections::VecDeque::new());

fn key(frame: u64) {
    let mut keyed = KEYED.lock().unwrap_or_else(|e| e.into_inner());
    if keyed.len() == 64 {
        keyed.pop_front();
    }
    keyed.push_back(frame);
}

fn keyed(n: u64) -> bool {
    KEYED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|&k| n.checked_sub(k).is_some_and(|d| d < span()))
}

/// The first frame of the F9/timed span frame `n` is in, if any.
#[cfg(feature = "diagnostics")]
fn span_start(n: u64) -> Option<u64> {
    KEYED.lock().unwrap_or_else(|e| e.into_inner()).iter().copied().filter(|&k| n.checked_sub(k).is_some_and(|d| d < span())).max()
}

/// Whether `what` is dumped for the first time in the span frame `n` is
/// in (`VICTORY_DUMP_ONCE=1`: each pass dumped once per F9 press, not in every
/// frame of a span; without it, always).
#[cfg(feature = "diagnostics")]
fn first_dump_in_span(n: u64, what: &str) -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var("VICTORY_DUMP_ONCE").is_ok_and(|v| v.trim() == "1")) {
        return true;
    }
    let Some(start) = span_start(n) else { return true };
    static DONE: std::sync::Mutex<Vec<(u64, String)>> = std::sync::Mutex::new(Vec::new());
    let mut done = DONE.lock().unwrap_or_else(|e| e.into_inner());
    if done.iter().any(|(s, w)| *s == start && w == what) {
        return false;
    }
    done.retain(|(s, _)| *s + 64 > start);
    done.push((start, what.to_owned()));
    true
}

/// With a capture folder, F9 pressed (with one of this process's windows in
/// front) at the present of `frame` makes the next frame a [`listed`] one:
/// its picture, its ops list and, with `VICTORY_DUMP_TARGETS`, what it
/// renders to, saved: a defect seen while playing (the purple foliage that
/// shows from some angles) caught at the frame it shows in.
#[cfg(windows)]
fn key_pressed(frame: u64) {
    use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F9};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    static DOWN: AtomicBool = AtomicBool::new(false);
    if folder().is_none() {
        return;
    }
    // SAFETY: a plain query.
    let down = unsafe { GetAsyncKeyState(i32::from(VK_F9)) } < 0;
    if DOWN.swap(down, Relaxed) || !down {
        return;
    }
    let mut process = 0;
    // SAFETY: plain queries; room for the answer.
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut process) };
    if process == std::process::id() {
        key(frame + 1);
        tracing::info!(frame = frame + 1, "F9: this frame is captured (picture, ops list, targets with VICTORY_DUMP_TARGETS)");
    }
}

/// `VICTORY_CAPTURE_AT_MS` (comma-separated milliseconds after the world's
/// first frame, [`world_started`]): at the present of the first frame past
/// each, the next frame is captured as F9 would ([`KEYED`]). With
/// `VICTORY_INPUT_REPLAY` the same moments of the same walk in every run.
fn timed(frame: u64) {
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    static AT: std::sync::OnceLock<Vec<u64>> = std::sync::OnceLock::new();
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let at = AT.get_or_init(|| {
        let mut list: Vec<u64> = std::env::var("VICTORY_CAPTURE_AT_MS")
            .unwrap_or_default()
            .split(',')
            .filter_map(|v| v.trim().parse().ok())
            .collect();
        list.sort_unstable();
        list
    });
    let (Some(start), Some(&due)) = (world_started(), at.get(NEXT.load(Relaxed))) else { return };
    if folder().is_some() && start.elapsed().as_millis() as u64 >= due {
        NEXT.fetch_add(1, Relaxed);
        key(frame + 1);
        tracing::info!(frame = frame + 1, ms = due, "a timed capture (VICTORY_CAPTURE_AT_MS)");
    }
}

/// `VICTORY_CAPTURE_TRIGGER=<file>`: when that file appears, it is removed
/// and the next frame captured as F9 would (with no window focus needed).
fn triggered(frame: u64) {
    static FILE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    let Some(path) = FILE.get_or_init(|| std::env::var_os("VICTORY_CAPTURE_TRIGGER").map(Into::into)) else { return };
    if folder().is_some() && std::fs::remove_file(path).is_ok() {
        key(frame + 1);
        tracing::info!(frame = frame + 1, "a triggered capture (VICTORY_CAPTURE_TRIGGER)");
    }
}

/// At the present of `frame`: the draws run on the host so far.
pub(crate) fn frame_drew(frame: u64, drawn: u64) {
    use std::sync::atomic::Ordering::Relaxed;
    #[cfg(windows)]
    key_pressed(frame);
    timed(frame);
    triggered(frame);
    #[cfg(feature = "diagnostics")]
    crate::renderdoc::at_present();
    #[cfg(feature = "diagnostics")]
    reread_probes(frame);
    #[cfg(feature = "diagnostics")]
    recheck_words(frame);
    #[cfg(feature = "diagnostics")]
    watch_sites_line(frame);
    #[cfg(feature = "diagnostics")]
    present_sleep();
    write_counts(frame);
    #[cfg(feature = "diagnostics")]
    mesh_stats::at_present(frame);
    #[cfg(feature = "diagnostics")]
    who::at_present(frame);
    #[cfg(feature = "diagnostics")]
    write_dumps(frame);
    static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let before = LAST.swap(drawn, Relaxed);
    if drawn.saturating_sub(before) >= 500 {
        let _ = WORLD_AT.get_or_init(std::time::Instant::now);
    }
    if world_on()
        && drawn.saturating_sub(before) >= 500
        && WORLD.compare_exchange(u64::MAX, frame, Relaxed, Relaxed).is_ok()
    {
        tracing::info!(frame, "the first frame of 500 draws or more: frames {} and {} are captured", frame + 2, frame + 6);
        #[cfg(feature = "diagnostics")]
        if let Some(folder) = folder() {
            let lines: Vec<String> = std::mem::take(&mut *RING.lock().unwrap_or_else(|e| e.into_inner())).into();
            let _ = std::fs::write(folder.join("before-world-ops.txt"), lines.join("
"));
        }
    }
}

/// Whether frame `n` is saved.
pub(crate) fn wanted(n: u64) -> bool {
    // Before the world, with `VICTORY_CAPTURE_WORLD`: every 16th frame too
    // (small pictures of menus, loading screens and title cards).
    let before_world = world_on() && n.is_multiple_of(16) && WORLD.load(std::sync::atomic::Ordering::Relaxed) == u64::MAX;
    // In the world: every 8th frame's picture, a timeline of the opening
    // cutscene (its shots do not fall on the same frame numbers from run to
    // run, so single frames cannot be compared between runs).
    let first = WORLD.load(std::sync::atomic::Ordering::Relaxed);
    let in_world = world_on() && first != u64::MAX && n > first && (n - first).is_multiple_of(8);
    folder().is_some() && (n.is_power_of_two() || listed(n) || before_world || in_world)
}

/// Saves 8-bit RGBA or BGRA rows (`pitch` bytes apart) as frame `n`.
pub(crate) fn save(n: u64, width: u32, height: u32, pitch: usize, bgra: bool, rows: &[u8]) {
    save_as(&format!("frame-{n}"), width, height, pitch, bgra, rows);
}

/// Saves 8-bit RGBA or BGRA rows as `<name>.png`, at half size.
fn save_as(name: &str, width: u32, height: u32, pitch: usize, bgra: bool, rows: &[u8]) {
    let Some(folder) = folder() else { return };
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    // Filter byte 0 per row, then RGB of every other pixel of every other row.
    let mut raw = Vec::with_capacity((w as usize * 3 + 1) * h as usize);
    for y in 0..h as usize {
        raw.push(0);
        let row = &rows[(2 * y).min(height as usize - 1) * pitch..];
        for x in 0..w as usize {
            let p = &row[(2 * x).min(width as usize - 1) * 4..][..3];
            if bgra {
                raw.extend([p[2], p[1], p[0]]);
            } else {
                raw.extend_from_slice(p);
            }
        }
    }
    let png = encode(w, h, &raw);
    let path = folder.join(format!("{name}.png"));
    match std::fs::write(&path, png) {
        Ok(()) => tracing::info!(path = %path.display(), "frame saved"),
        Err(e) => tracing::warn!(error = %e, path = %path.display(), "frame not saved"),
    }
}

/// A PNG of `width * height` pixels of 8-bit RGB.
pub(crate) fn png(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let row = width as usize * 3;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgb.chunks_exact(row.max(1)) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    encode(width, height, &raw)
}

/// A PNG of rows of RGB, each behind its filter byte.
fn encode(w: u32, h: u32, raw: &[u8]) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend(w.to_be_bytes());
    header.extend(h.to_be_bytes());
    header.extend([8, 2, 0, 0, 0]); // 8-bit RGB
    chunk(&mut png, b"IHDR", &header);
    let mut compressed = vec![0u8; zlib_rs::compress_bound(raw.len())];
    let (idat, rc) = zlib_rs::compress_slice(&mut compressed, raw, zlib_rs::DeflateConfig::new(3));
    debug_assert_eq!(rc, zlib_rs::ReturnCode::Ok);
    chunk(&mut png, b"IDAT", idat);
    chunk(&mut png, b"IEND", &[]);
    png
}

fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend((data.len() as u32).to_be_bytes());
    let start = png.len();
    png.extend(kind);
    png.extend(data);
    let crc = crc32(&png[start..]);
    png.extend(crc.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

/// Records a copy of `texture` (in the copy-source state) into a
/// readback buffer.
pub(crate) fn read_back(
    device: &ID3D12Device,
    list: &ID3D12GraphicsCommandList,
    texture: &ID3D12Resource,
) -> Option<(ID3D12Resource, D3D12_PLACED_SUBRESOURCE_FOOTPRINT)> {
    read_back_of(device, list, texture, 0)
}

/// As [`read_back`], of subresource `sub` (the stencil plane of a
/// one-level, one-slice depth-stencil texture is subresource 1).
fn read_back_of(
    device: &ID3D12Device,
    list: &ID3D12GraphicsCommandList,
    texture: &ID3D12Resource,
    sub: u32,
) -> Option<(ID3D12Resource, D3D12_PLACED_SUBRESOURCE_FOOTPRINT)> {
    let mut footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
    let mut total = 0;
    // SAFETY: a live texture; outputs for one subresource.
    unsafe {
        let desc = texture.GetDesc();
        device.GetCopyableFootprints(
            &desc,
            sub,
            1,
            0,
            Some(&mut footprint),
            None,
            None,
            Some(&mut total),
        );
    }
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_READBACK,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: total,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let mut buffer: Option<ID3D12Resource> = None;
    // SAFETY: plain creation; an open list; locations naming live resources.
    unsafe {
        device
            .CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut buffer,
            )
            .ok()?;
        let buffer = buffer?;
        let to = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::transmute_copy(&buffer),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: footprint,
            },
        };
        let from = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::transmute_copy(texture),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: sub,
            },
        };
        list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
        Some((buffer, footprint))
    }
}

/// Saves a frame read back by [`read_back`] (8-bit color only).
pub(crate) fn save_read_back(
    frame: u64,
    buffer: &ID3D12Resource,
    footprint: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
) {
    let f = footprint.Footprint;
    let bgra = match f.Format {
        DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => false,
        DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => true,
        other => {
            tracing::warn!(format = other.0, "frame not saved: not 8-bit color");
            return;
        }
    };
    let mut data = std::ptr::null_mut();
    // SAFETY: the copy finished (every submission is waited for); the
    // mapping spans the footprint's rows.
    unsafe {
        if buffer.Map(0, None, Some(&mut data)).is_err() {
            return;
        }
        let len = f.RowPitch as usize * f.Height as usize;
        let rows = std::slice::from_raw_parts(data.cast::<u8>(), len);
        save(frame, f.Width, f.Height, f.RowPitch as usize, bgra, rows);
        buffer.Unmap(0, None);
    }
}

/// With `VICTORY_DUMP_TARGETS` set: title textures the frames listed in
/// `VICTORY_CAPTURE_FRAMES` render to or write, in first-use order, saved
/// at that frame's present as `frame-<n>-<i>-<resource>-<w>x<h>-f<format>.png`
/// (what each holds at the end of the frame; formats normalized to show
/// structure: floats scaled by their largest value, depth by its range).
/// The host copies are held: the title may release a target before the
/// frame is presented.
static TARGETS: std::sync::Mutex<Vec<(usize, std::sync::Arc<crate::resource::HostTexture>)>> =
    std::sync::Mutex::new(Vec::new());

fn dump_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| folder().is_some() && crate::switch::on("VICTORY_DUMP_TARGETS"))
}

/// Notes a title texture the frame being built renders to or writes.
pub(crate) fn note_target(resource: crate::object::This) {
    if resource.is_null() || !dump_on() || !listed(crate::queue::presented() + 1) {
        return;
    }
    let mut targets = TARGETS.lock().unwrap_or_else(|e| e.into_inner());
    if !targets.iter().any(|&(r, _)| r == resource as usize)
        && let Some(copy) = crate::resource::host_copy(resource)
    {
        targets.push((resource as usize, copy));
    }
}

/// At the present of frame `frame`: saves the noted targets.
pub(crate) fn dump_targets(frame: u64) {
    use std::sync::atomic::Ordering::Relaxed;
    let targets = std::mem::take(&mut *TARGETS.lock().unwrap_or_else(|e| e.into_inner()));
    let Some(host) = crate::host::host() else { return };
    for (i, (resource, copy)) in targets.iter().enumerate() {
        let texture = &copy.texture;
        // SAFETY: a live resource.
        let d = unsafe { texture.GetDesc() };
        if d.SampleDesc.Count > 1 || d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D {
            continue;
        }
        // Every submission has run; the dump's list moves it there and back.
        let state = copy.state.load(Relaxed);
        dump_one(host, texture, state, frame, i, *resource as crate::object::This, &d);
    }
}

fn dump_one(
    host: &crate::host::Host,
    texture: &ID3D12Resource,
    state: u32,
    frame: u64,
    i: usize,
    resource: crate::object::This,
    d: &D3D12_RESOURCE_DESC,
) {
    let Ok(list) = host.new_list() else { return };
    let state = D3D12_RESOURCE_STATES(crate::host::legal_state(texture, state) as i32);
    let copy = D3D12_RESOURCE_STATE_COPY_SOURCE;
    // SAFETY: an open list of ours and a live texture.
    let read = unsafe {
        if state != copy {
            list.list
                .ResourceBarrier(&[crate::host::transition(texture, state, copy)]);
        }
        let read = read_back(&host.device, &list.list, texture);
        // The stencil plane of a depth-stencil target, which the title's
        // lighting reads (material class in the low bits; 0x08 "inside a
        // room", which the lamps of a room are multiplied by): the plane
        // after the depth plane of a one-level, one-slice texture.
        let stencil = (matches!(d.Format.0, 19..=22 | 44..=47)
            && d.MipLevels == 1
            && d.DepthOrArraySize == 1)
            .then(|| read_back_of(&host.device, &list.list, texture, 1))
            .flatten();
        if state != copy {
            list.list
                .ResourceBarrier(&[crate::host::transition(texture, copy, state)]);
        }
        (read, stencil)
    };
    let (read, stencil) = read;
    list.close();
    host.execute(&[&list]);
    if let Some((buffer, footprint)) = stencil {
        stencil_values(&buffer, &footprint, frame, resource);
    }
    let Some((buffer, footprint)) = read else { return };
    let f = footprint.Footprint;
    let mut data = std::ptr::null_mut();
    // SAFETY: the copy finished; the mapping spans the footprint's rows.
    unsafe {
        if buffer.Map(0, None, Some(&mut data)).is_err() {
            return;
        }
        let pitch = f.RowPitch as usize;
        let rows = std::slice::from_raw_parts(data.cast::<u8>(), pitch * f.Height as usize);
        // A 24-bit depth plane comes back as 32-bit words: which depth values
        // (low 24 bits) and which top bytes it holds, the most common first.
        if matches!(d.Format.0, 44..=47) {
            let mut depths = std::collections::HashMap::<u32, u32>::new();
            let mut tops = std::collections::HashMap::<u32, u32>::new();
            for y in 0..f.Height as usize {
                for x in 0..f.Width as usize {
                    let at = y * pitch + x * 4;
                    let w = u32::from_le_bytes(rows[at..at + 4].try_into().unwrap_or([0; 4]));
                    *depths.entry(w & 0xff_ffff).or_default() += 1;
                    *tops.entry(w >> 24).or_default() += 1;
                }
            }
            let most = |m: std::collections::HashMap<u32, u32>| {
                let mut v: Vec<(u32, u32)> = m.into_iter().collect();
                v.sort_by_key(|e| std::cmp::Reverse(e.1));
                (v.len(), v.into_iter().take(12).collect::<Vec<_>>())
            };
            tracing::info!(?resource, depths = ?most(depths), top_bytes = ?most(tops), "24-bit depth plane values (value, texels)");
        }
        if let Some(rgba) = to_rgba(f.Format.0 as u32, f.Width as usize, f.Height as usize, pitch, rows) {
            let name = format!(
                "frame-{frame}-{i:02}-{resource:?}-{}x{}-f{}",
                d.Width, d.Height, d.Format.0
            );
            save_as(&name, f.Width, f.Height, f.Width as usize * 4, false, &rgba);
        } else {
            tracing::info!(format = f.Format.0, ?resource, "target not dumped: format");
        }
        buffer.Unmap(0, None);
    }
}

/// Logs which values a stencil plane read back by [`read_back_of`] holds
/// (the most common first), how many texels have each of its bits, and the
/// box of the texels with bit 3 (0x08).
fn stencil_values(
    buffer: &ID3D12Resource,
    footprint: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    frame: u64,
    resource: crate::object::This,
) {
    let f = footprint.Footprint;
    let (w, h, pitch) = (f.Width as usize, f.Height as usize, f.RowPitch as usize);
    let mut data = std::ptr::null_mut();
    // SAFETY: the copy finished; the mapping spans the footprint's rows of
    // one byte a texel, the last row without its padding.
    unsafe {
        if h == 0 || w == 0 || buffer.Map(0, None, Some(&mut data)).is_err() {
            return;
        }
        let rows = std::slice::from_raw_parts(data.cast::<u8>(), pitch * (h - 1) + w);
        let mut values = [0u32; 256];
        let mut bits = [0u32; 8];
        // Left, top, right, bottom of the texels with bit 3.
        let mut inside = [u32::MAX, u32::MAX, 0, 0];
        for y in 0..h {
            for (x, &v) in rows[y * pitch..y * pitch + w].iter().enumerate() {
                values[usize::from(v)] += 1;
                for (bit, count) in bits.iter_mut().enumerate() {
                    *count += u32::from(v >> bit & 1);
                }
                if v & 8 != 0 {
                    inside = [
                        inside[0].min(x as u32),
                        inside[1].min(y as u32),
                        inside[2].max(x as u32),
                        inside[3].max(y as u32),
                    ];
                }
            }
        }
        let mut most: Vec<(usize, u32)> = values
            .iter()
            .copied()
            .enumerate()
            .filter(|v| v.1 != 0)
            .collect();
        most.sort_by_key(|v| std::cmp::Reverse(v.1));
        most.truncate(16);
        // The plane itself, grey = the stencil value, to read the material
        // class of a surface that lights wrongly against its neighbours'.
        let mut grey = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for &v in &rows[y * pitch..y * pitch + w] {
                grey.extend_from_slice(&[v, v, v, 255]);
            }
        }
        save_as(&format!("frame-{frame}-stencil-{resource:?}-{w}x{h}"), w as u32, h as u32, w * 4, false, &grey);
        tracing::info!(
            frame,
            ?resource,
            width = w,
            height = h,
            values = ?most,
            texels_with_bit = ?bits,
            bit3_box = ?inside,
            "stencil plane values (value, texels)"
        );
        buffer.Unmap(0, None);
    }
}

fn half(h: u16) -> f32 {
    let (sign, exp, man) = ((h >> 15) as u32, ((h >> 10) & 0x1f) as i32, (h & 0x3ff) as f32);
    let v = match exp {
        0 => man * 2f32.powi(-24),
        31 => f32::INFINITY,
        e => (1.0 + man / 1024.0) * 2f32.powi(e - 15),
    };
    if sign == 1 { -v } else { v }
}

/// A float packed in `bits` with a 5-bit exponent and `man` mantissa bits
/// (the 11- and 10-bit floats of R11G11B10).
fn small_float(bits: u32, man: u32) -> f32 {
    let (exp, m) = ((bits >> man) & 0x1f, (bits & ((1 << man) - 1)) as f32);
    match exp {
        0 => m / (1 << man) as f32 * 2f32.powi(-14),
        31 => f32::INFINITY,
        e => (1.0 + m / (1 << man) as f32) * 2f32.powi(e as i32 - 15),
    }
}

/// Converts host-format rows to opaque 8-bit RGBA for the RGB dump. Byte
/// colors and packed 16-bit BGR use their full channel range. Other formats
/// scale to the highest finite value, with single channels shown in grey
/// from their lowest to highest value. Returns `None` for unhandled formats.
fn to_rgba(format: u32, w: usize, h: usize, pitch: usize, rows: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(w * h * 4);
    let px = |x: usize, y: usize, n: usize| &rows[y * pitch + x * n..][..n];
    let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[2 * i], b[2 * i + 1]]);
    let u32_at = |b: &[u8], i: usize| u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap_or([0; 4]));
    // Channel values as floats, then scaled.
    type Decode = Box<dyn Fn(&[u8]) -> [f32; 3]>;
    let (n, decode): (usize, Decode) = match format {
        27..=32 => {
            for y in 0..h {
                for x in 0..w {
                    let p = px(x, y, 4);
                    out.extend([p[0], p[1], p[2], 255]);
                }
            }
            return Some(out);
        }
        // B8G8R8A8 and B8G8R8X8 (89 is R10G10B10_XR_BIAS_A2, below).
        87 | 88 | 90..=93 => {
            for y in 0..h {
                for x in 0..w {
                    let p = px(x, y, 4);
                    out.extend([p[2], p[1], p[0], 255]);
                }
            }
            return Some(out);
        }
        // B5G6R5, B5G5R5A1 and B4G4R4A4 store blue in the low bits.
        85 | 86 | 115 => {
            let (rb_bits, green_bits) = match format {
                85 => (5, 6),
                86 => (5, 5),
                _ => (4, 4),
            };
            let unorm8 = |value: u16, bits: u32| {
                let max = (1u16 << bits) - 1;
                (((value & max) * 255 + max / 2) / max) as u8
            };
            for y in 0..h {
                for x in 0..w {
                    let word = u16_at(px(x, y, 2), 0);
                    out.extend([
                        unorm8(word >> (rb_bits + green_bits), rb_bits),
                        unorm8(word >> rb_bits, green_bits),
                        unorm8(word, rb_bits),
                        255,
                    ]);
                }
            }
            return Some(out);
        }
        23..=25 | 89 => (4, Box::new(move |b| {
            let v = u32_at(b, 0);
            [(v & 0x3ff) as f32, (v >> 10 & 0x3ff) as f32, (v >> 20 & 0x3ff) as f32]
        })),
        26 => (4, Box::new(move |b| {
            let v = u32_at(b, 0);
            [small_float(v & 0x7ff, 6), small_float(v >> 11 & 0x7ff, 6), small_float(v >> 22, 5)]
        })),
        9..=10 => (8, Box::new(move |b| [half(u16_at(b, 0)), half(u16_at(b, 1)), half(u16_at(b, 2))])),
        11..=14 => (8, Box::new(move |b| [0, 1, 2].map(|i| f32::from(u16_at(b, i))))),
        33..=34 => (4, Box::new(move |b| [half(u16_at(b, 0)), half(u16_at(b, 1)), 0.0])),
        1..=2 => (16, Box::new(move |b| {
            [0, 1, 2].map(|i| f32::from_bits(u32_at(b, i)))
        })),
        15..=16 => (8, Box::new(move |b| [f32::from_bits(u32_at(b, 0)), f32::from_bits(u32_at(b, 1)), 0.0])),
        35..=38 => (4, Box::new(move |b| [f32::from(u16_at(b, 0)), f32::from(u16_at(b, 1)), 0.0])),
        48..=52 => (2, Box::new(move |b| [f32::from(b[0]), f32::from(b[1]), 0.0])),
        // Single channels (and depth planes): shown as grey.
        39..=43 => (4, Box::new(move |b| [f32::from_bits(u32_at(b, 0)); 3])),
        19..=22 => (8, Box::new(move |b| [f32::from_bits(u32_at(b, 0)); 3])),
        44..=47 => (4, Box::new(move |b| [(u32_at(b, 0) & 0xff_ffff) as f32; 3])),
        53..=54 => (2, Box::new(move |b| [half(u16_at(b, 0)); 3])),
        55..=59 => (2, Box::new(move |b| [f32::from(u16_at(b, 0)); 3])),
        60..=65 => (1, Box::new(move |b| [f32::from(b[0]); 3])),
        _ => return None,
    };
    let values: Vec<[f32; 3]> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| decode(px(x, y, n)))
        .collect();
    let grey = matches!(format, 19..=22 | 39..=47 | 53..=65);
    let finite = values.iter().flatten().copied().filter(|v| v.is_finite());
    let (low, high) = if grey {
        finite.fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(v), h.max(v)))
    } else {
        (0.0, finite.fold(0.0f32, f32::max))
    };
    let range = (high - low).max(1e-20);
    // How much of the target holds anything: a lit scene and one bright
    // lamp on black have the same highest value.
    let above = values.iter().filter(|v| v.iter().any(|c| c.is_finite() && *c > low)).count();
    let nonzero_percent = above as f32 * 100.0 / values.len().max(1) as f32;
    tracing::info!(format, low, high, nonzero_percent, "target values");
    // Small float colour targets (the 1x1 luminance and exposure targets):
    // every texel's four channels as numbers. The picture has no alpha and
    // eight bits a channel.
    if matches!(format, 1..=2 | 9..=10) && w * h <= 16 {
        for y in 0..h {
            for x in 0..w {
                let b = px(x, y, n);
                let texel: [f32; 4] = if n == 16 {
                    [0, 1, 2, 3].map(|i| f32::from_bits(u32_at(b, i)))
                } else {
                    [0, 1, 2, 3].map(|i| half(u16_at(b, i)))
                };
                tracing::info!(format, x, y, ?texel, "target texel");
            }
        }
    }
    // R32_UINT targets hold packed light (the title reads them back as
    // R9G9B9E5): as floats their low, high and share above the lowest say
    // nothing. How many texels hold any bit, and the highest channel value
    // of the packed reading.
    if format == 42 {
        let (mut set, mut packed_high) = (0usize, 0f32);
        for y in 0..h {
            for x in 0..w {
                let v = u32_at(px(x, y, 4), 0);
                set += usize::from(v != 0);
                let scale = 2f32.powi((v >> 27) as i32 - 24);
                for k in 0..3 {
                    packed_high = packed_high.max((v >> (9 * k) & 0x1ff) as f32 * scale);
                }
            }
        }
        let nonzero_bits_percent = set as f32 * 100.0 / (w * h).max(1) as f32;
        tracing::info!(format, nonzero_bits_percent, packed_high, "target values as packed light");
        // The same words as plain numbers, by class. Tables of indices and
        // masks say nothing as floats or as packed light: in the title's
        // shadow-mask table 0 is a block in full shadow, a word up to 255
        // one value for the block, above that a block of the atlas, and all
        // ones a texel never written.
        let mut classes = [0usize; 6];
        let mut highest_below_ones = 0u32;
        for y in 0..h {
            for x in 0..w {
                let v = u32_at(px(x, y, 4), 0);
                classes[match v {
                    0 => 0,
                    1..=15 => 1,
                    16..=255 => 2,
                    256..=0x00ff_ffff => 3,
                    u32::MAX => 5,
                    _ => 4,
                }] += 1;
                if v != u32::MAX {
                    highest_below_ones = highest_below_ones.max(v);
                }
            }
        }
        tracing::info!(
            format,
            width = w,
            height = h,
            zero = classes[0],
            one_to_15 = classes[1],
            to_255 = classes[2],
            to_24_bits = classes[3],
            larger = classes[4],
            all_ones = classes[5],
            highest_below_ones,
            "target values as words"
        );
    }
    // Float colours (scene light) go through a cube root: scaled linearly,
    // a night scene under one lamp of 50 units came out all black.
    let curved = matches!(format, 1..=2 | 9..=10 | 26);
    for v in values {
        let c = v.map(|c| {
            let t = ((c - low) / range).clamp(0.0, 1.0);
            ((if curved { t.cbrt() } else { t }) * 255.0) as u8
        });
        out.extend([c[0], c[1], c[2], 255]);
    }
    Some(out)
}

/// Pixels per side of the grid [`probe`] reads.
const GRID: u32 = 4;
/// Bytes between the grid's pixels in the readback buffer (placement
/// alignment).
const SLOT: u64 = 512;

/// Records copies of a grid of pixels of `texture` (8-bit color, in the
/// copy-source state) into a readback buffer, to tell black frames.
pub(crate) fn probe(
    device: &ID3D12Device,
    list: &ID3D12GraphicsCommandList,
    texture: &ID3D12Resource,
) -> Option<ID3D12Resource> {
    // SAFETY: a live resource.
    let d = unsafe { texture.GetDesc() };
    if !matches!(d.Format.0, 27..=29 | 87 | 90 | 91) {
        return None;
    }
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: SLOT * u64::from(GRID * GRID),
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_READBACK,
        ..Default::default()
    };
    let mut buffer: Option<ID3D12Resource> = None;
    // SAFETY: plain creation; an open list; locations naming live resources.
    unsafe {
        device
            .CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut buffer,
            )
            .ok()?;
        let buffer = buffer?;
        for i in 0..GRID * GRID {
            let (x, y) = (
                (2 * (i % GRID) + 1) * d.Width as u32 / (2 * GRID),
                (2 * (i / GRID) + 1) * d.Height / (2 * GRID),
            );
            let to = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(&buffer),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: SLOT * u64::from(i),
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: d.Format,
                            Width: 1,
                            Height: 1,
                            Depth: 1,
                            RowPitch: 256,
                        },
                    },
                },
            };
            let from = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(texture),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            let pixel = D3D12_BOX {
                left: x,
                top: y,
                front: 0,
                right: x + 1,
                bottom: y + 1,
                back: 1,
            };
            list.CopyTextureRegion(&to, 0, 0, 0, &from, Some(&pixel));
        }
        Some(buffer)
    }
}

/// Whether every pixel [`probe`] read is (near) black.
pub(crate) fn black(buffer: &ID3D12Resource) -> bool {
    let mut data = std::ptr::null_mut();
    // SAFETY: the copies finished (every submission is waited for).
    unsafe {
        if buffer.Map(0, None, Some(&mut data)).is_err() {
            return false;
        }
        let bytes =
            std::slice::from_raw_parts(data.cast::<u8>(), (SLOT * u64::from(GRID * GRID)) as usize);
        let level = bytes
            .chunks(SLOT as usize)
            .flat_map(|pixel| pixel[..3].iter().copied())
            .max()
            .unwrap_or(0);
        buffer.Unmap(0, None);
        let mut levels = LEVELS.lock().unwrap_or_else(|e| e.into_inner());
        if levels.len() == 12 {
            levels.pop_front();
        }
        levels.push_back(level);
        level < 8
    }
}

/// The brightest probed channel of the last frames, oldest first: a fade
/// darkens over several, a glitch drops at once.
static LEVELS: std::sync::Mutex<std::collections::VecDeque<u8>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

pub(crate) fn recent_levels() -> Vec<u8> {
    let levels = LEVELS.lock().unwrap_or_else(|e| e.into_inner());
    levels.iter().copied().collect()
}

/// An indirect execution of a [`listed`] frame whose arguments, as the card
/// ran them (after the title's culling wrote them and the clamp bounded
/// them), are read back: `frame-<n>-counts.txt` says per execution how many
/// records did work and how many instances (draws) or groups (dispatches)
/// they asked for. Objects the title's GPU culling leaves out show as
/// executions with fewer instances than in a frame where they are drawn.
#[cfg(feature = "diagnostics")]
struct Probe {
    frame: u64,
    kind: u32,
    stride: u32,
    max: u32,
    name: String,
    /// A leaked buffer the read-back lands in (`HostList::defer_read_back`).
    at: usize,
    len: usize,
    /// The arguments' title address, and title memory there when the
    /// execution was recorded and again at its frame's present: against what
    /// the card ran, whether the card's copy was the title's (a write after
    /// the refresh, or through an address the guards do not watch, leaves
    /// the card running older arguments).
    title_at: u64,
    title_recorded: Vec<u8>,
    title_presented: Option<Vec<u8>>,
    /// The present `title_presented` was read at.
    title_when: u64,
}

/// At the present of `frame`: title memory at the arguments of that
/// frame's probes, read again ([`Probe::title_presented`]).
#[cfg(feature = "diagnostics")]
fn reread_probes(frame: u64) {
    let mut probes = PROBES.lock().unwrap_or_else(|e| e.into_inner());
    for p in probes.iter_mut().filter(|p| p.frame <= frame) {
        let mut now = vec![0u8; p.len];
        if !crate::host::read_into(p.title_at as *const u8, &mut now) {
            continue;
        }
        // The first read, and afterwards the one with the most work: what
        // shaders wrote there reaches title memory at all, and when.
        let total = |b: &[u8]| work_of(b, p.kind, p.stride, p.max).1;
        let better = p.title_presented.as_ref().is_none_or(|old| total(&now) > total(old));
        if better {
            p.title_presented = Some(now);
            p.title_when = frame;
        }
    }
}

/// The work `bytes` of `max` records of `kind`, `stride` bytes apart, ask
/// for: records with work, total, the first eight.
#[cfg(feature = "diagnostics")]
fn work_of(bytes: &[u8], kind: u32, stride: u32, max: u32) -> (u32, u64, Vec<u64>) {
    let words: Vec<u32> = bytes.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
    let per = (stride / 4).max(1) as usize;
    let (mut active, mut total, mut shown) = (0u32, 0u64, Vec::new());
    for record in words.chunks(per).take(max.max(1) as usize) {
        let work = match kind {
            // Vertex or index count, instance count.
            0 | 1 => u64::from(record.first().copied().unwrap_or(0).min(1)) * u64::from(record.get(1).copied().unwrap_or(0)),
            // Groups x, y, z.
            _ => record.iter().take(3).map(|&v| u64::from(v)).product(),
        };
        if work > 0 {
            active += 1;
            total += work;
        }
        if shown.len() < 8 {
            shown.push(work);
        }
    }
    (active, total, shown)
}

#[cfg(feature = "diagnostics")]
static PROBES: std::sync::Mutex<Vec<Probe>> = std::sync::Mutex::new(Vec::new());

#[cfg(feature = "diagnostics")]
thread_local! {
    /// The title's code that issued the call being recorded ([`note_callers`]).
    static CALLERS: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// In a [`listed`] frame: the first four return addresses on this thread's
/// stack that lie outside our own executable (the title's code, as
/// `module+offset`), for [`probe_counts`] to name the code that issued an
/// indirect execution: a draw is usually issued through a shared helper,
/// the decision of whether to draw sits a frame or two up.
#[cfg(feature = "diagnostics")]
pub(crate) fn note_callers() {
    if folder().is_none() || !listed(crate::queue::presented() + 1) {
        return;
    }
    let found = title_return_addresses(6);
    CALLERS.with(|c| *c.borrow_mut() = found.iter().map(|a| format!("+{a:#x}")).collect::<Vec<_>>().join("<"));
}

/// The first `n` return addresses into the title's image (`victory-image-*`)
/// on this thread's stack, as offsets into it, by a scan of the stack: the
/// unwinder loses its way at the generated stubs between the title's calls
/// and ours. A value counts when it points into the title's image right
/// after a call instruction (`call rel32`, or `call` through a register or
/// memory: 0xFF with ModRM reg 2, two to seven bytes back).
#[cfg(feature = "diagnostics")]
fn title_return_addresses(n: usize) -> Vec<usize> {
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW,
        GetModuleHandleExW,
    };
    use windows_sys::Win32::System::Threading::GetCurrentThreadStackLimits;
    // The title's image: (base, end), found once from any address in it.
    static TITLE: std::sync::OnceLock<std::sync::Mutex<Option<(usize, usize)>>> = std::sync::OnceLock::new();
    let title = TITLE.get_or_init(|| std::sync::Mutex::new(crate::TITLE_IMAGE.get().copied()));
    let module_of = |addr: usize| -> Option<(usize, bool)> {
        let mut module = std::ptr::null_mut();
        // SAFETY: looks up the module holding the address, no reference.
        let ok = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                addr as *const u16,
                &mut module,
            )
        };
        if ok == 0 {
            return None;
        }
        let mut name = [0u16; 260];
        // SAFETY: room for the name.
        let len = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) } as usize;
        let is_title = String::from_utf16_lossy(&name[..len.min(name.len())]).contains("victory-image");
        Some((module as usize, is_title))
    };
    let (mut low, mut high) = (0usize, 0usize);
    // SAFETY: two out values.
    unsafe { GetCurrentThreadStackLimits(&mut low, &mut high) };
    let here = std::ptr::addr_of!(low) as usize & !7;
    // The stack this code runs on may be a fiber's, outside the thread's
    // limits: the scan stops at the end of the memory region `here` lies
    // in, and 64 KiB past it at most.
    let region_end = {
        use windows_sys::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery};
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: a query of our own stack's region into a local.
        let got = unsafe { VirtualQuery(here as *const _, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
        if got == 0 { here } else { mbi.BaseAddress as usize + mbi.RegionSize }
    };
    let high_scan = region_end.min(here + (64 << 10));
    let (mut in_title, mut scanned) = (0usize, 0usize);
    let mut out = Vec::new();
    let mut at = here;
    while at + 8 <= high_scan && out.len() < n {
        scanned += 1;
        // SAFETY: inside this thread's stack, below its base.
        let v = unsafe { std::ptr::read_volatile(at as *const usize) };
        at += 8;
        let known = *title.lock().unwrap_or_else(|e| e.into_inner());
        let base = match known {
            Some((base, end)) if v >= base + 8 && v < end => base,
            Some(_) => continue,
            None => match module_of(v) {
                Some((base, true)) => {
                    // SAFETY: the image's headers are mapped: e_lfanew, then SizeOfImage.
                    let size = unsafe {
                        let pe = base + std::ptr::read_unaligned((base + 0x3c) as *const u32) as usize;
                        std::ptr::read_unaligned((pe + 0x50) as *const u32) as usize
                    };
                    *title.lock().unwrap_or_else(|e| e.into_inner()) = Some((base, base + size));
                    base
                }
                _ => continue,
            },
        };
        in_title += 1;
        // SAFETY: the seven bytes before an address in the title's mapped
        // image (at least eight past its base).
        let before: [u8; 7] = unsafe { std::ptr::read_unaligned((v - 7) as *const [u8; 7]) };
        let call_rel = before[2] == 0xE8;
        let call_ind = (2..=7).any(|k| before[7 - k] == 0xFF && (before[8 - k] >> 3) & 7 == 2);
        if call_rel || call_ind {
            out.push(v - base);
        }
    }
    static TOLD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if TOLD.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 3 {
        let title = *title.lock().unwrap_or_else(|e| e.into_inner());
        tracing::info!(
            here = format_args!("{here:#x}"),
            thread_stack = format_args!("{low:#x}-{high:#x}"),
            region_end = format_args!("{region_end:#x}"),
            title = ?title.map(|(a, b)| (format!("{a:#x}"), format!("{b:#x}"))),
            scanned,
            in_title,
            found = ?out.iter().map(|a| format!("+{a:#x}")).collect::<Vec<_>>(),
            "callers: the stack scan"
        );
    }
    out
}

/// After `list` recorded an `ExecuteIndirect` of `max` records of `kind`
/// (0 draw, 1 indexed, 2 dispatch, 10 mesh) `stride` bytes apart from
/// `args` at `at`, left in the indirect-argument state: copies them for
/// [`write_counts`]. In [`listed`] frames only.
#[cfg(feature = "diagnostics")]
pub(crate) fn probe_counts(list: &crate::host::HostList, args: &ID3D12Resource, at: u64, (kind, stride, max): (u32, u32, u32), name: &str, title_at: u64) {
    let frame = crate::queue::presented() + 1;
    if folder().is_none() || !listed(frame) {
        return;
    }
    let bytes = u64::from(stride) * u64::from(max.max(1));
    if bytes == 0 || bytes > 1 << 20 {
        return;
    }
    let Some(host) = crate::host::host() else { return };
    let Some(buffer) = host.read_back_buffer(bytes) else { return };
    use crate::host::transition;
    // SAFETY: an open list; the arguments buffer is left readable as
    // arguments by the execution just recorded and put back so after.
    unsafe {
        list.list.ResourceBarrier(&[transition(args, D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
        list.list.CopyBufferRegion(&buffer, 0, args, at, bytes);
        list.list.ResourceBarrier(&[transition(args, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT)]);
    }
    let landing: &'static mut [u8] = Box::leak(vec![0u8; bytes as usize].into_boxed_slice());
    list.defer_read_back(buffer, landing.as_mut_ptr(), [bytes as usize, 1, bytes as usize]);
    let callers = CALLERS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    let mut title_recorded = vec![0u8; bytes as usize];
    if !crate::host::read_into(title_at as *const u8, &mut title_recorded) {
        title_recorded.clear();
    }
    PROBES.lock().unwrap_or_else(|e| e.into_inner()).push(Probe {
        frame,
        kind,
        stride,
        max,
        name: format!("{name} from {callers}"),
        at: landing.as_ptr() as usize,
        len: bytes as usize,
        title_at,
        title_recorded,
        title_presented: None,
        title_when: 0,
    });
}

/// At the present of `presented`: the probes of frames eight or more
/// before it (their lists have run) written to `frame-<n>-counts.txt`, one
/// line per execution in recording order, and their buffers freed.
#[cfg(feature = "diagnostics")]
fn write_counts(presented: u64) {
    let done: Vec<Probe> = {
        let mut probes = PROBES.lock().unwrap_or_else(|e| e.into_inner());
        if probes.is_empty() {
            return;
        }
        let (done, left) = std::mem::take(&mut *probes).into_iter().partition(|p| p.frame + 8 <= presented);
        *probes = left;
        done
    };
    let Some(folder) = folder() else { return };
    let mut files: std::collections::BTreeMap<u64, String> = std::collections::BTreeMap::new();
    for p in done {
        // SAFETY: leaked in `probe_counts` with this length; read once, freed here.
        let bytes = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(p.at as *mut u8, p.len)) };
        let (active, total, shown) = work_of(&bytes, p.kind, p.stride, p.max);
        let title_total = |b: &[u8]| if b.is_empty() { "unread".to_owned() } else { work_of(b, p.kind, p.stride, p.max).1.to_string() };
        let line = format!(
            "kind {} max {} active {active} total {total} first {shown:?} title_recorded {} title_presented {} (+{}) at {:#x} {}\n",
            p.kind,
            p.max,
            title_total(&p.title_recorded),
            p.title_presented.as_deref().map_or("unread".to_owned(), title_total),
            p.title_when.saturating_sub(p.frame),
            p.title_at,
            p.name
        );
        files.entry(p.frame).or_default().push_str(&line);
    }
    for (frame, text) in files {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(folder.join(format!("frame-{frame}-counts.txt"))) {
            let _ = f.write_all(text.as_bytes());
        }
    }
}

/// `VICTORY_PRESENT_SLEEP=<file>`: the milliseconds in the file (read again
/// every second) are slept at each present: the title's frames made longer
/// with nothing else changed.
#[cfg(feature = "diagnostics")]
fn present_sleep() {
    static FILE: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    static MS: std::sync::Mutex<(Option<std::time::Instant>, u64)> = std::sync::Mutex::new((None, 0));
    let Some(file) = FILE.get_or_init(|| std::env::var_os("VICTORY_PRESENT_SLEEP").map(Into::into)) else { return };
    let ms = {
        let mut m = MS.lock().unwrap_or_else(|e| e.into_inner());
        if m.0.is_none_or(|t| t.elapsed().as_secs() >= 1) {
            m.0 = Some(std::time::Instant::now());
            let v = std::fs::read_to_string(file).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            if v != m.1 {
                tracing::info!(ms = v, "VICTORY_PRESENT_SLEEP: now");
            }
            m.1 = v;
        }
        m.1
    };
    if ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(ms.min(2000)));
    }
}

/// `VICTORY_WATCH_PIPES=<hash>,..`: the 4 KiB pages of the small raw
/// buffers (`VICTORY_OPS_WORDS`) that draws of those pipelines read are
/// watched: the title's write faults on them ([`watched_fault`]) name the
/// title code that wrote them.
#[cfg(feature = "diagnostics")]
static WATCHED_PAGES: std::sync::Mutex<Option<std::collections::HashSet<usize>>> = std::sync::Mutex::new(None);
#[cfg(feature = "diagnostics")]
static WATCHED_RANGES: std::sync::Mutex<std::collections::BTreeMap<usize, usize>> = std::sync::Mutex::new(std::collections::BTreeMap::new());
#[cfg(feature = "diagnostics")]
static WATCHED_ANY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "diagnostics")]
static WATCH_SITES: std::sync::Mutex<Option<std::collections::HashMap<String, u64>>> = std::sync::Mutex::new(None);

/// Whether draws of pipeline `name` have their buffers' pages watched.
#[cfg(feature = "diagnostics")]
pub(crate) fn watch_pipe(name: &str) -> bool {
    static PIPES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    PIPES
        .get_or_init(|| std::env::var("VICTORY_WATCH_PIPES").unwrap_or_default().split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
        .iter()
        .any(|p| name.contains(p.as_str()))
}

/// Whether a buffer read through `label` (`<shader> s<slot>+<offset>`) is
/// watched: `VICTORY_WATCH_SLOTS` (`s1+72,s1+56` unless given) names the
/// table places; any shader.
#[cfg(feature = "diagnostics")]
pub(crate) fn watch_slot(label: &str) -> bool {
    static SLOTS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    SLOTS
        .get_or_init(|| {
            std::env::var("VICTORY_WATCH_SLOTS")
                .unwrap_or_else(|_| "s1+72,s1+56".into())
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .iter()
        .any(|s| label == s || label.ends_with(&format!(" {s}")))
}

/// Adds the pages of `[at, at + len)` to the watched ones.
#[cfg(feature = "diagnostics")]
pub(crate) fn watch_range(at: u64, len: usize) {
    let mut w = WATCHED_PAGES.lock().unwrap_or_else(|e| e.into_inner());
    let set = w.get_or_insert_with(Default::default);
    if set.len() > 20_000 {
        return;
    }
    for page in (at as usize >> 12)..=((at as usize + len.max(1) - 1) >> 12) {
        set.insert(page);
    }
    let mut r = WATCHED_RANGES.lock().unwrap_or_else(|e| e.into_inner());
    if r.len() < 50_000 {
        let e = r.entry(at as usize).or_insert(0);
        *e = (*e).max(at as usize + len);
    }
    WATCHED_ANY.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// From the write-fault handler (the faulting thread): a write at `at` by
/// the instruction at `rip`. On a watched page: the instruction and the
/// title's return addresses on this stack are counted.
#[cfg(feature = "diagnostics")]
pub(crate) fn watched_fault(at: usize, rip: usize) {
    if !WATCHED_ANY.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let hit = WATCHED_PAGES.try_lock().ok().and_then(|w| w.as_ref().map(|s| s.contains(&(at >> 12)))).unwrap_or(false);
    if !hit {
        return;
    }
    // Exactly inside one of the buffers, or only on its page.
    let exact = WATCHED_RANGES.try_lock().ok().is_some_and(|r| r.range(..=at).next_back().is_some_and(|(_, &end)| at < end));
    let base = crate::TITLE_IMAGE.get().map_or(0, |t| t.0);
    let here = if base != 0 && rip > base { format!("+{:#x}", rip - base) } else { format!("{rip:#x}") };
    let callers: Vec<String> = title_return_addresses(8).iter().map(|a| format!("+{a:#x}")).collect();
    let key = format!("{} {here} <{}", if exact { "IN" } else { "page" }, callers.join("<"));
    if let Ok(mut s) = WATCH_SITES.try_lock() {
        *s.get_or_insert_with(Default::default).entry(key).or_insert(0) += 1;
    }
}

/// The watched pages' writers so far, most frequent first, logged.
#[cfg(feature = "diagnostics")]
fn watch_sites_line(presented: u64) {
    if !presented.is_multiple_of(64) {
        return;
    }
    let Some(sites) = WATCH_SITES.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
    let mut v: Vec<(String, u64)> = sites.into_iter().collect();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (site, n) in v.iter().take(12) {
        tracing::info!(presented, n, site = site.as_str(), "VICTORY_WATCH_PIPES: a writer of the watched buffers");
    }
}

/// Small raw buffers' bytes as copied at a list's submission, to be read
/// again at later presents ([`recheck_words`]): address, bytes, the frame
/// presented when submitted, its tag, the presents seen since.
#[cfg(feature = "diagnostics")]
static SUBMITTED_WORDS: std::sync::Mutex<Vec<(u64, Vec<u8>, u64, String, u32)>> = std::sync::Mutex::new(Vec::new());

/// A small raw buffer's bytes as the GPU got them at a list's submission.
#[cfg(feature = "diagnostics")]
pub(crate) fn words_submitted(at: u64, bytes: Vec<u8>, tag: &str) {
    let mut s = SUBMITTED_WORDS.lock().unwrap_or_else(|e| e.into_inner());
    if s.len() < 200_000 {
        s.push((at, bytes, crate::queue::presented(), tag.to_owned(), 0));
    }
}

/// At each present: the buffers submitted before it read again; those that
/// title memory now holds otherwise are counted by how many presents after
/// the submission they changed (0: before the present that followed it -
/// the title wrote them after submitting the list that reads them). One
/// line, and the first few as ops lines (`AFTER-SUBMIT`).
#[cfg(feature = "diagnostics")]
fn recheck_words(presented: u64) {
    let mut s = SUBMITTED_WORDS.lock().unwrap_or_else(|e| e.into_inner());
    if s.is_empty() {
        return;
    }
    let (mut checked, mut changed_first, mut changed_later, mut shown) = (0u64, 0u64, 0u64, 0u32);
    s.retain_mut(|(at, bytes, when, tag, seen)| {
        if *when >= presented {
            return true;
        }
        checked += 1;
        let mut now = vec![0u8; bytes.len()];
        if !crate::host::read_into(*at as *const u8, &mut now) {
            return false;
        }
        if &now != bytes {
            if *seen == 0 {
                changed_first += 1;
                if shown < 40 {
                    shown += 1;
                    let w = |b: &[u8]| b.chunks_exact(4).map(|w| format!("{:08x}", u32::from_le_bytes([w[0], w[1], w[2], w[3]]))).collect::<Vec<_>>().join(" ");
                    let (a, b) = (w(bytes), w(&now));
                    op(0, || format!("AFTER-SUBMIT {tag} {at:#x} submitted [{a}] present [{b}]"));
                }
            } else {
                changed_later += 1;
            }
            return false;
        }
        *seen += 1;
        *seen < 3
    });
    if checked > 0 {
        tracing::info!(
            presented,
            checked,
            changed_before_next_present = changed_first,
            changed_later = changed_later,
            "small raw buffers read again after their list's submission (VICTORY_OPS_WORDS)"
        );
    }
}

/// `VICTORY_SKIP_ARGS=<file>`: lines `<lo> <hi>` of title addresses (hex):
/// indirect executions whose arguments start in `[lo, hi)` are dropped;
/// lines of one word: draws (direct or indirect) whose pipeline name holds
/// it are dropped ([`skip_name`]). The file is read again at most every
/// 200 ms, so what is dropped can change while the camera holds still:
/// what a record or a pipeline draws on screen.
#[cfg(feature = "diagnostics")]
pub(crate) fn skip_args(arguments: u64) -> bool {
    skip_list(|ranges, _, _| ranges.iter().any(|&(lo, hi)| arguments >= lo && arguments < hi))
}

/// [`skip_args`]'s lines `<name> <lo> <hi>` (decimal): the executions of
/// pipeline `name` numbered `[lo, hi)` in recording order within each host
/// list (from 0 at its reset) are dropped. Returns the execution's number
/// when a line names the pipeline (for the ops line) and whether it is
/// dropped.
#[cfg(feature = "diagnostics")]
pub(crate) fn skip_ordinal(list: &crate::host::HostList, name: &str) -> (Option<u64>, bool) {
    let rules: Vec<(String, u64, u64)> =
        skip_list(|_, _, ords| ords.iter().filter(|o| name.contains(o.0.as_str())).cloned().collect());
    if rules.is_empty() {
        return (None, false);
    }
    static COUNTS: std::sync::Mutex<Option<std::collections::HashMap<(usize, String), u64>>> = std::sync::Mutex::new(None);
    let key_name = rules[0].0.clone();
    // A key of the list's own set, new after each reset.
    let reset_key = usize::MAX - 0x5000 - (key_name.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize)) & 0xfff);
    let fresh = list.first_use(reset_key);
    let mut counts = COUNTS.lock().unwrap_or_else(|e| e.into_inner());
    let counts = counts.get_or_insert_with(Default::default);
    let n = counts.entry((std::ptr::from_ref(list) as usize, key_name)).or_insert(0);
    if fresh {
        *n = 0;
    }
    let ordinal = *n;
    *n += 1;
    (Some(ordinal), rules.iter().any(|&(_, lo, hi)| ordinal >= lo && ordinal < hi))
}

/// [`skip_args`]'s one-word lines: whether a draw of pipeline `name` is dropped.
#[cfg(feature = "diagnostics")]
pub(crate) fn skip_name(name: &str) -> bool {
    skip_list(|_, names, _| names.iter().any(|n| name.contains(n.as_str())))
}

#[cfg(feature = "diagnostics")]
fn skip_list<T: Default>(test: impl FnOnce(&[(u64, u64)], &[String], &[(String, u64, u64)]) -> T) -> T {
    type Lists = (Option<std::time::Instant>, Vec<(u64, u64)>, Vec<String>, Vec<(String, u64, u64)>);
    static FILE: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    static LISTS: std::sync::Mutex<Lists> = std::sync::Mutex::new((None, Vec::new(), Vec::new(), Vec::new()));
    let Some(file) = FILE.get_or_init(|| std::env::var_os("VICTORY_SKIP_ARGS").map(Into::into)) else {
        return T::default();
    };
    let mut r = LISTS.lock().unwrap_or_else(|e| e.into_inner());
    if r.0.is_none_or(|t| t.elapsed().as_millis() >= 200) {
        r.0 = Some(std::time::Instant::now());
        let hex = |w: &str| u64::from_str_radix(w.trim_start_matches("0x"), 16).ok();
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let (mut ranges, mut names, mut ords) = (Vec::new(), Vec::new(), Vec::new());
        for line in text.lines() {
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                [lo, hi] => {
                    if let (Some(lo), Some(hi)) = (hex(lo), hex(hi)) {
                        ranges.push((lo, hi));
                    }
                }
                [name] => names.push((*name).to_owned()),
                [name, lo, hi] => {
                    if let (Ok(lo), Ok(hi)) = (lo.parse(), hi.parse()) {
                        ords.push(((*name).to_owned(), lo, hi));
                    }
                }
                _ => {}
            }
        }
        if ranges != r.1 || names != r.2 || ords != r.3 {
            tracing::info!(
                ranges = ?ranges.iter().map(|(a, b)| format!("{a:#x}-{b:#x}")).collect::<Vec<_>>(),
                ?names,
                ?ords,
                "VICTORY_SKIP_ARGS: dropped now"
            );
        }
        r.1 = ranges;
        r.2 = names;
        r.3 = ords;
    }
    test(&r.1, &r.2, &r.3)
}

/// The ranges of `VICTORY_DUMP` (`<address>,<len>` pairs, `;` apart,
/// decimal or 0x hex) and the pipeline hashes of `VICTORY_DUMP_AFTER`;
/// with `VICTORY_DUMP_FILE=<file>` the ranges are that file's lines
/// `<address> <len>` (hex), read again every 200 ms.
#[cfg(feature = "diagnostics")]
fn dump_spec() -> (Vec<(u64, u64)>, Vec<String>) {
    let fixed = dump_spec_fixed();
    static FILE: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    let Some(file) = FILE.get_or_init(|| std::env::var_os("VICTORY_DUMP_FILE").map(Into::into)) else {
        return (fixed.0.clone(), fixed.1.clone());
    };
    type Now = (Option<std::time::Instant>, Vec<(u64, u64)>, Vec<String>);
    static NOW: std::sync::Mutex<Now> = std::sync::Mutex::new((None, Vec::new(), Vec::new()));
    let mut now = NOW.lock().unwrap_or_else(|e| e.into_inner());
    if now.0.is_none_or(|t| t.elapsed().as_millis() >= 200) {
        now.0 = Some(std::time::Instant::now());
        let hex = |w: &str| u64::from_str_radix(w.trim_start_matches("0x"), 16).ok();
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let ranges: Vec<(u64, u64)> = text
            .lines()
            .filter(|l| !l.starts_with("after"))
            .filter_map(|l| {
                let mut w = l.split_whitespace();
                Some((hex(w.next()?)?, hex(w.next()?)?))
            })
            .filter(|&(_, l)| l > 0 && l <= 8 << 20)
            .collect();
        // Lines `after <hash> ...`: the passes dumped after (in place of VICTORY_DUMP_AFTER's).
        let after: Vec<String> = text
            .lines()
            .filter_map(|l| l.strip_prefix("after"))
            .flat_map(|l| l.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
            .collect();
        now.2 = if after.is_empty() { fixed.1.clone() } else { after };
        if ranges != now.1 {
            tracing::info!(ranges = ?ranges.iter().map(|(a, l)| format!("{a:#x}+{l:#x}")).collect::<Vec<_>>(), "VICTORY_DUMP_FILE: ranges now");
        }
        now.1 = ranges;
    }
    (now.1.clone(), now.2.clone())
}

#[cfg(feature = "diagnostics")]
fn dump_spec_fixed() -> &'static (Vec<(u64, u64)>, Vec<String>) {
    static SPEC: std::sync::OnceLock<(Vec<(u64, u64)>, Vec<String>)> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| {
        let num = |w: &str| -> Option<u64> {
            let w = w.trim();
            match w.strip_prefix("0x") {
                Some(h) => u64::from_str_radix(h, 16).ok(),
                None => w.parse().ok(),
            }
        };
        let ranges = std::env::var("VICTORY_DUMP")
            .unwrap_or_default()
            .split(';')
            .filter_map(|r| {
                let (a, l) = r.split_once(',')?;
                Some((num(a)?, num(l)?))
            })
            .filter(|&(_, l)| l > 0 && l <= 8 << 20)
            .collect();
        let after = std::env::var("VICTORY_DUMP_AFTER")
            .unwrap_or_default()
            .split(',')
            .map(|h| h.trim().to_owned())
            .filter(|h| !h.is_empty())
            .collect();
        (ranges, after)
    })
}

#[cfg(feature = "diagnostics")]
struct Dump {
    frame: u64,
    seq: u64,
    list: usize,
    what: String,
    address: u64,
    at: usize,
    len: usize,
}

#[cfg(feature = "diagnostics")]
static DUMPS: std::sync::Mutex<Vec<Dump>> = std::sync::Mutex::new(Vec::new());

/// In a [`listed`] frame, after a dispatch or indirect execution of
/// pipeline `name` was recorded in `list`: when `name` holds one of
/// `VICTORY_DUMP_AFTER`'s hashes, `VICTORY_DUMP`'s ranges as the GPU holds
/// them at this place of the list ([`dump_ranges`]).
#[cfg(feature = "diagnostics")]
pub(crate) fn dump_after(list: &crate::host::HostList, name: &str) {
    let (ranges, after) = dump_spec();
    if ranges.is_empty() || !after.iter().any(|h| h == "*" || name.contains(h.as_str())) {
        return;
    }
    let frame = crate::queue::presented() + 1;
    if folder().is_none() || !listed(frame) || !first_dump_in_span(frame, name) {
        return;
    }
    dump_ranges(list, &format!("after {name}"));
}

/// In a [`listed`] frame, before the first indirect draw in `list` whose
/// arguments lie in one of `VICTORY_DUMP`'s ranges: those ranges as that
/// draw finds them (after every pass before it in the list).
#[cfg(feature = "diagnostics")]
pub(crate) fn dump_before_reader(list: &crate::host::HostList, arguments: u64, name: &str) {
    let (ranges, _) = dump_spec();
    if ranges.is_empty() || !ranges.iter().any(|&(a, l)| arguments >= a && arguments < a + l) {
        return;
    }
    let frame = crate::queue::presented() + 1;
    if folder().is_none() || !listed(frame) || !first_dump_in_span(frame, "before the first reader") {
        return;
    }
    static SEEN: std::sync::Mutex<Vec<(u64, usize)>> = std::sync::Mutex::new(Vec::new());
    let key = (frame, std::ptr::from_ref(list) as usize);
    {
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if seen.contains(&key) {
            return;
        }
        seen.retain(|s| s.0 + 16 > frame);
        seen.push(key);
    }
    dump_ranges(list, &format!("before the first reader {name} at {arguments:#x}"));
}

/// Copies each of `VICTORY_DUMP`'s ranges from its host copy at this place
/// of `list` (everything before it done and seen; nothing refreshed from
/// title memory: what the GPU holds) into a read-back buffer, and notes an
/// operation `DUMP <seq>` in the frame's operations.
#[cfg(feature = "diagnostics")]
fn dump_ranges(list: &crate::host::HostList, what: &str) {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let frame = crate::queue::presented() + 1;
    if folder().is_none() || !listed(frame) {
        return;
    }
    let Some(host) = crate::host::host() else { return };
    let me = std::ptr::from_ref(list) as usize;
    for (address, len) in dump_spec().0 {
        let Some(shadow) = crate::shadow::peek(address, len) else {
            op(me, || format!("DUMP none of {address:#x}+{len:#x} (no host copy) {what}"));
            continue;
        };
        let Some(buffer) = host.read_back_buffer(len) else { return };
        // SAFETY: an open list; the host copy outlives the list (copies
        // are kept), the range inside it; the read-back buffer is kept by
        // the deferred read.
        unsafe {
            crate::host::place::wait_for_all(host, &list.list);
            list.list.CopyBufferRegion(&buffer, 0, &shadow.buffer, shadow.offset(address), len);
            crate::host::place::wait_for_all(host, &list.list);
        }
        let landing: &'static mut [u8] = Box::leak(vec![0u8; len as usize].into_boxed_slice());
        list.defer_read_back(buffer, landing.as_mut_ptr(), [len as usize, 1, len as usize]);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        op(me, || format!("DUMP {seq} of {address:#x}+{len:#x} {what}"));
        // Title memory of the same range as the title holds it now (while the
        // list is recorded), beside the GPU's: `frame-<n>-title-<seq>.bin`.
        if let Some(folder) = folder() {
            let mut title = vec![0u8; len as usize];
            let read = crate::host::read_into(address as *const u8, &mut title);
            let _ = std::fs::write(folder.join(format!("frame-{frame}-title-{seq}.bin")), &title);
            if !read {
                op(me, || format!("DUMP {seq}: title memory of {address:#x}+{len:#x} not readable (zeros written)"));
            }
        }
        DUMPS.lock().unwrap_or_else(|e| e.into_inner()).push(Dump {
            frame,
            seq,
            list: me,
            what: what.to_owned(),
            address,
            at: landing.as_ptr() as usize,
            len: len as usize,
        });
    }
}

/// At the present of `presented`: the dumps of frames eight or more before
/// it appended to `frame-<n>-dump.bin`, each listed in `frame-<n>-dumps.txt`
/// (sequence, list, range, offset in the .bin, what), and freed.
#[cfg(feature = "diagnostics")]
fn write_dumps(presented: u64) {
    use std::io::Write;
    let done: Vec<Dump> = {
        let mut dumps = DUMPS.lock().unwrap_or_else(|e| e.into_inner());
        if dumps.is_empty() {
            return;
        }
        let (done, left) = std::mem::take(&mut *dumps).into_iter().partition(|d| d.frame + 8 <= presented);
        *dumps = left;
        done
    };
    let Some(folder) = folder() else { return };
    for d in done {
        // SAFETY: leaked in `dump_ranges` with this length; read once, freed here.
        let bytes = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(d.at as *mut u8, d.len)) };
        let bin = folder.join(format!("frame-{}-dump.bin", d.frame));
        let offset = std::fs::metadata(&bin).map_or(0, |m| m.len());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&bin) {
            let _ = f.write_all(&bytes);
        }
        let index = folder.join(format!("frame-{}-dumps.txt", d.frame));
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(index) {
            let _ = writeln!(f, "{} list {:#x} range {:#x}+{:#x} offset {offset} {}", d.seq, d.list, d.address, d.len, d.what);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc_matches_the_png_check_value() {
        assert_eq!(super::crc32(b"IEND"), 0xae42_6082);
    }

    #[test]
    fn packed_bgr_targets_preserve_color_and_skip_row_padding() {
        // Each 2x2 target has red, green, blue and a mixed color. The first
        // row has a padding word, and alpha bits do not affect the RGB dump.
        for (format, words, mixed) in [
            (85, [0xf800u16, 0x07e0, 0x001f, 0x8408], [132, 130, 66, 255]),
            (86, [0x7c00, 0x03e0, 0x801f, 0xc208], [132, 132, 66, 255]),
            (115, [0x0f00, 0x00f0, 0xf00f, 0xf841], [136, 68, 17, 255]),
        ] {
            let [red, green, blue, blend] = words;
            let rows = [red, green, 0xcafe, blue, blend]
                .map(u16::to_le_bytes)
                .concat();
            let expected = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], mixed].concat();
            assert_eq!(
                super::to_rgba(format, 2, 2, 6, &rows),
                Some(expected),
                "format {format}"
            );
        }
    }

    /// R16G16B16A16_UNORM is integers, not half floats; 89 is 10:10:10:2,
    /// not B8G8R8A8.
    #[test]
    fn unorm16_and_xr_bias_targets_decode_as_their_formats() {
        // Red 0x8000 (a half-float -0), green 0x4000, blue 0.
        let unorm16 = [0x00, 0x80, 0x00, 0x40, 0, 0, 0xff, 0xff];
        assert_eq!(super::to_rgba(11, 1, 1, 8, &unorm16), Some(vec![255, 127, 0, 255]));
        // Red 0x3ff, green 0x1ff, blue 0, alpha 3.
        let xr = (0x3ffu32 | 0x1ff << 10 | 3 << 30).to_le_bytes();
        assert_eq!(super::to_rgba(89, 1, 1, 4, &xr), Some(vec![255, 127, 0, 255]));
        // B8G8R8A8 still swaps to RGBA.
        assert_eq!(super::to_rgba(87, 1, 1, 4, &[1, 2, 3, 4]), Some(vec![3, 2, 1, 255]));
    }
}

/// `VICTORY_MESH_STATS=all|<hash>[,<hash>...]` (diagnostics): every mesh draw
/// run through `ExecuteIndirect` whose mesh shader matches (`all`: every one)
/// is wrapped in a pipeline-statistics query, and every ~5 s the totals per
/// mesh shader are logged: draws, mesh-shader groups run (`MSInvocations`),
/// primitives they emitted (`MSPrimitives`), primitives the rasterizer was
/// given and kept after clipping (`CInvocations`, `CPrimitives`) and pixel
/// shader invocations (`PSInvocations`). For telling a draw whose shader
/// emits nothing from one whose primitives are lost after it (the per-slot
/// mesh draws of the culling pass's empty detail levels, 27 Nov).
#[cfg(feature = "diagnostics")]
pub(crate) mod mesh_stats {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};
    use windows::Win32::Graphics::Direct3D12::*;

    const SLOTS: u32 = 1 << 16;
    const SIZE: u64 = std::mem::size_of::<D3D12_QUERY_DATA_PIPELINE_STATISTICS1>() as u64;

    struct Stats {
        heap: ID3D12QueryHeap,
        buffer: ID3D12Resource,
        mapped: usize,
        /// Occlusion queries (samples that passed every test and were not
        /// discarded), one u64 a slot.
        occ: ID3D12QueryHeap,
        occ_buffer: ID3D12Resource,
        occ_mapped: usize,
    }
    // SAFETY: the heap and buffer are free-threaded D3D12 objects; the
    // mapping is only read.
    unsafe impl Send for Stats {}
    // SAFETY: as above.
    unsafe impl Sync for Stats {}

    fn filter() -> Option<&'static Vec<String>> {
        static F: OnceLock<Option<Vec<String>>> = OnceLock::new();
        F.get_or_init(|| {
            let v = std::env::var("VICTORY_MESH_STATS").ok()?;
            Some(v.split([',', ';', ' ']).map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).collect())
        })
        .as_ref()
    }

    fn stats() -> Option<&'static Stats> {
        static S: OnceLock<Option<Stats>> = OnceLock::new();
        S.get_or_init(|| {
            let host = crate::host::host()?;
            let desc = D3D12_QUERY_HEAP_DESC { Type: D3D12_QUERY_HEAP_TYPE_PIPELINE_STATISTICS1, Count: SLOTS, NodeMask: 0 };
            let mut heap: Option<ID3D12QueryHeap> = None;
            // SAFETY: plain creation.
            if let Err(e) = unsafe { host.device.CreateQueryHeap(&desc, &mut heap) } {
                tracing::warn!(error = %e, "VICTORY_MESH_STATS: no pipeline-statistics-1 query heap (mesh statistics unsupported?)");
                return None;
            }
            let buffer = host.read_back_buffer(u64::from(SLOTS) * SIZE)?;
            let mut mapped = std::ptr::null_mut();
            // SAFETY: a read-back buffer, mapped once for the process.
            unsafe { buffer.Map(0, None, Some(&mut mapped)) }.ok()?;
            let desc = D3D12_QUERY_HEAP_DESC { Type: D3D12_QUERY_HEAP_TYPE_OCCLUSION, Count: SLOTS, NodeMask: 0 };
            let mut occ: Option<ID3D12QueryHeap> = None;
            // SAFETY: plain creation.
            unsafe { host.device.CreateQueryHeap(&desc, &mut occ) }.ok()?;
            let occ_buffer = host.read_back_buffer(u64::from(SLOTS) * 8)?;
            let mut occ_mapped = std::ptr::null_mut();
            // SAFETY: as above.
            unsafe { occ_buffer.Map(0, None, Some(&mut occ_mapped)) }.ok()?;
            tracing::info!("VICTORY_MESH_STATS: pipeline statistics and occlusion on for mesh draws");
            Some(Stats { heap: heap?, buffer, mapped: mapped as usize, occ: occ?, occ_buffer, occ_mapped: occ_mapped as usize })
        })
        .as_ref()
    }

    /// The mesh shader hash in a pipeline name (`... mesh <16 hex> ...`).
    fn mesh_of(name: &str) -> Option<&str> {
        let at = name.find("mesh ")? + 5;
        name.get(at..at + 16)
    }

    static NEXT: AtomicU32 = AtomicU32::new(0);
    static PENDING: Mutex<Vec<(u32, u64, String)>> = Mutex::new(Vec::new());

    /// Before a mesh draw: begins a query when the draw's mesh shader is asked for.
    pub(crate) fn begin(list: &crate::host::HostList, name: &str) -> Option<u32> {
        let f = filter()?;
        let mesh = mesh_of(name)?;
        if !f.iter().any(|h| h == "all" || mesh.starts_with(h.as_str())) {
            return None;
        }
        let s = stats()?;
        let slot = NEXT.fetch_add(1, Ordering::Relaxed) % SLOTS;
        // SAFETY: an open list; a slot of our heap.
        unsafe {
            list.list.BeginQuery(&s.heap, D3D12_QUERY_TYPE_PIPELINE_STATISTICS1, slot);
            list.list.BeginQuery(&s.occ, D3D12_QUERY_TYPE_OCCLUSION, slot);
        }
        Some(slot)
    }

    /// After that draw: ends the query and resolves it into the read-back buffer.
    pub(crate) fn end(list: &crate::host::HostList, slot: u32, name: &str) {
        let Some(s) = stats() else { return };
        // SAFETY: an open list; the query begun in `begin`; the buffer stays
        // in COPY_DEST (a read-back heap).
        unsafe {
            list.list.EndQuery(&s.occ, D3D12_QUERY_TYPE_OCCLUSION, slot);
            list.list.ResolveQueryData(&s.occ, D3D12_QUERY_TYPE_OCCLUSION, slot, 1, &s.occ_buffer, u64::from(slot) * 8);
            list.list.EndQuery(&s.heap, D3D12_QUERY_TYPE_PIPELINE_STATISTICS1, slot);
            list.list.ResolveQueryData(&s.heap, D3D12_QUERY_TYPE_PIPELINE_STATISTICS1, slot, 1, &s.buffer, u64::from(slot) * SIZE);
        }
        let key = format!("mesh {} {}", mesh_of(name).unwrap_or("?"), name.split(", pixel ").nth(1).map_or("", |p| p.get(..16).unwrap_or("")));
        PENDING.lock().unwrap_or_else(|e| e.into_inner()).push((slot, crate::queue::presented() + 1, key));
    }

    #[derive(Default, Clone, Copy)]
    struct Total {
        draws: u64,
        groups: u64,
        ms_prims: u64,
        c_inv: u64,
        c_prims: u64,
        ps: u64,
        samples: u64,
        empty_draws: u64,
        discarded_draws: u64,
    }

    static TOTALS: Mutex<Option<HashMap<String, Total>>> = Mutex::new(None);
    static LOGGED: AtomicU64 = AtomicU64::new(0);

    /// At a present: queries of frames eight or more before it are read and
    /// added up; every 150 presents the totals so far are logged.
    pub(crate) fn at_present(presented: u64) {
        let Some(s) = stats() else { return };
        let done: Vec<(u32, u64, String)> = {
            let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
            if p.is_empty() {
                return;
            }
            let (done, left) = std::mem::take(&mut *p).into_iter().partition(|e| e.1 + 8 <= presented);
            *p = left;
            done
        };
        let mut totals = TOTALS.lock().unwrap_or_else(|e| e.into_inner());
        let totals = totals.get_or_insert_with(HashMap::new);
        for (slot, _, key) in done {
            // SAFETY: inside the mapped buffer; written by the card for a list that has run.
            let q = unsafe { std::ptr::read_unaligned((s.mapped as *const u8).add((u64::from(slot) * SIZE) as usize).cast::<D3D12_QUERY_DATA_PIPELINE_STATISTICS1>()) };
            let t = totals.entry(key).or_default();
            t.draws += 1;
            t.groups += q.MSInvocations;
            t.ms_prims += q.MSPrimitives;
            t.c_inv += q.CInvocations;
            t.c_prims += q.CPrimitives;
            t.ps += q.PSInvocations;
            // SAFETY: inside the mapped occlusion buffer.
            let samples = unsafe { std::ptr::read_unaligned((s.occ_mapped as *const u8).add(slot as usize * 8).cast::<u64>()) };
            t.samples += samples;
            if q.PSInvocations > 0 && samples == 0 {
                t.discarded_draws += 1;
            }
            if q.MSInvocations > 0 && q.PSInvocations == 0 {
                t.empty_draws += 1;
            }
        }
        if presented.saturating_sub(LOGGED.load(Ordering::Relaxed)) < 150 {
            return;
        }
        LOGGED.store(presented, Ordering::Relaxed);
        let mut rows: Vec<(&String, &Total)> = totals.iter().collect();
        rows.sort_by(|a, b| b.1.groups.cmp(&a.1.groups));
        for (key, t) in rows.iter().take(60) {
            tracing::info!(
                draws = t.draws,
                groups = t.groups,
                ms_primitives = t.ms_prims,
                raster_in = t.c_inv,
                raster_kept = t.c_prims,
                pixel_invocations = t.ps,
                samples_landed = t.samples,
                draws_all_pixels_discarded = t.discarded_draws,
                draws_with_groups_but_no_pixels = t.empty_draws,
                "VICTORY_MESH_STATS {key}"
            );
        }
    }
}

/// `VICTORY_PIXEL_WHO=<file>` (diagnostics): lines `x y w h` (pixels of a
/// render target; `#` comments; read again every 200 ms). In an F9/listed
/// frame, after every draw whose first render target is large enough, those
/// boxes of that target are copied out; at the frame's end
/// `frame-N-who.txt` lists, per box and target, each draw that changed pixels
/// in it (how many, in recording order) with its pipeline name and what it
/// was: which draws wrote an object's pixels, without dropping any (27 Nov:
/// the bin, fence and bushes that vanish at middle distance).
#[cfg(feature = "diagnostics")]
pub(crate) mod who {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};
    use windows::Win32::Graphics::Direct3D12::*;

    /// Read-back room for one frame's copies.
    const ROOM: u64 = 512 << 20;

    struct Space {
        buffer: ID3D12Resource,
        mapped: usize,
    }
    // SAFETY: a free-threaded D3D12 resource; the mapping is only read.
    unsafe impl Send for Space {}
    // SAFETY: as above.
    unsafe impl Sync for Space {}

    fn space() -> Option<&'static Space> {
        static S: OnceLock<Option<Space>> = OnceLock::new();
        S.get_or_init(|| {
            let host = crate::host::host()?;
            let buffer = host.read_back_buffer(ROOM)?;
            let mut mapped = std::ptr::null_mut();
            // SAFETY: a read-back buffer mapped once for the process.
            unsafe { buffer.Map(0, None, Some(&mut mapped)) }.ok()?;
            Some(Space { buffer, mapped: mapped as usize })
        })
        .as_ref()
    }

    fn boxes() -> Vec<[u32; 4]> {
        static FILE: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
        let Some(file) = FILE.get_or_init(|| std::env::var_os("VICTORY_PIXEL_WHO").map(Into::into)) else {
            return Vec::new();
        };
        type Now = (Option<std::time::Instant>, Vec<[u32; 4]>);
        static NOW: Mutex<Now> = Mutex::new((None, Vec::new()));
        let mut now = NOW.lock().unwrap_or_else(|e| e.into_inner());
        if now.0.is_none_or(|t| t.elapsed().as_millis() >= 200) {
            now.0 = Some(std::time::Instant::now());
            let text = std::fs::read_to_string(file).unwrap_or_default();
            now.1 = text
                .lines()
                .map(|l| l.split('#').next().unwrap_or(""))
                .filter_map(|l| {
                    let v: Vec<u32> = l.split_whitespace().filter_map(|w| w.parse().ok()).collect();
                    (v.len() == 4 && v[2] > 0 && v[3] > 0 && v[2] * v[3] <= 256 * 256).then(|| [v[0], v[1], v[2], v[3]])
                })
                .collect();
        }
        now.1.clone()
    }

    /// Bytes a texel of `format` takes, for the formats render targets use.
    fn texel_bytes(format: i32) -> Option<u32> {
        Some(match format {
            1..=4 => 16,
            9..=22 => 8,
            23..=47 => 4,
            48..=60 => 2,
            61..=65 => 1,
            85..=93 => 4,
            _ => return None,
        })
    }

    struct Probe {
        frame: u64,
        list: usize,
        draw: u64,
        name: String,
        target: usize,
        boxed: [u32; 4],
        offset: u64,
        pitch: u32,
        texel: u32,
    }

    static PROBES: Mutex<Vec<Probe>> = Mutex::new(Vec::new());

    thread_local! {
        /// What the draw about to be recorded is (its arguments), for the listing.
        static WHAT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }

    /// Notes what the next draw on this thread is.
    pub(crate) fn note(what: String) {
        WHAT.with(|w| *w.borrow_mut() = what);
    }

    /// What [`note`] said, taken.
    pub(crate) fn take_note() -> String {
        WHAT.with(|w| std::mem::take(&mut *w.borrow_mut()))
    }
    static USED: AtomicU64 = AtomicU64::new(0);
    static FRAME_OF_USE: AtomicU64 = AtomicU64::new(0);
    static DRAWS: AtomicU64 = AtomicU64::new(0);

    /// After a draw: its first render target's boxes copied out.
    pub(crate) fn after_draw(list: &crate::host::HostList, targets: &[usize], name: &str, what: &str) {
        let frame = crate::queue::presented() + 1;
        if super::folder().is_none() || !super::listed(frame) {
            return;
        }
        let boxes = boxes();
        if boxes.is_empty() {
            return;
        }
        let Some(&target) = targets.first() else { return };
        let Some(texture) = crate::resource::host_made(target as crate::object::This) else { return };
        let Some(s) = space() else { return };
        // SAFETY: a live resource.
        let desc = unsafe { texture.GetDesc() };
        // A box of a texture can be copied only from a plain colour one:
        // depth-capable, multisampled or other textures are copied whole or
        // not at all (a list holding such a copy does not close).
        const DEPTH_FORMATS: [i32; 9] = [19, 20, 21, 22, 39, 40, 44, 45, 55];
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
            || desc.SampleDesc.Count != 1
            || desc.Flags.contains(D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL)
            || !desc.Flags.contains(D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET)
            || DEPTH_FORMATS.contains(&desc.Format.0)
        {
            return;
        }
        let Some(texel) = texel_bytes(desc.Format.0) else { return };
        if FRAME_OF_USE.swap(frame, Ordering::Relaxed) != frame {
            // The read-back room is reused only once every probe before has been read.
            if PROBES.lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
                USED.store(0, Ordering::Relaxed);
            }
            DRAWS.store(0, Ordering::Relaxed);
        }
        let draw = DRAWS.fetch_add(1, Ordering::Relaxed);
        let transition = crate::host::transition;
        for b in boxes {
            if u64::from(b[0] + b[2]) > desc.Width || b[1] + b[3] > desc.Height {
                continue;
            }
            let pitch = (b[2] * texel).next_multiple_of(256);
            let size = u64::from(pitch) * u64::from(b[3]);
            let offset = USED.fetch_add(size.next_multiple_of(512), Ordering::Relaxed);
            if offset + size > ROOM {
                return;
            }
            let dst = D3D12_TEXTURE_COPY_LOCATION {
                // SAFETY: a borrowed pointer the location never releases.
                pResource: unsafe { std::mem::transmute_copy(&s.buffer) },
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: offset,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT { Format: desc.Format, Width: b[2], Height: b[3], Depth: 1, RowPitch: pitch },
                    },
                },
            };
            let src = D3D12_TEXTURE_COPY_LOCATION {
                // SAFETY: as above (`texture` is already a reference to the
                // interface: its pointer, not the reference's address).
                pResource: unsafe { std::mem::transmute_copy(texture) },
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
            };
            let area = D3D12_BOX { left: b[0], top: b[1], front: 0, right: b[0] + b[2], bottom: b[1] + b[3], back: 1 };
            // SAFETY: an open list; a bound render target is in
            // RENDER_TARGET, and goes back there.
            unsafe {
                list.list.ResourceBarrier(&[transition(&texture, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
                list.list.CopyTextureRegion(&dst, 0, 0, 0, &src, Some(&area));
                list.list.ResourceBarrier(&[transition(&texture, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET)]);
            }
            PROBES.lock().unwrap_or_else(|e| e.into_inner()).push(Probe {
                frame,
                list: std::ptr::from_ref(list) as usize,
                draw,
                name: format!("{what} {name}"),
                target,
                boxed: b,
                offset,
                pitch,
                texel,
            });
        }
    }

    /// At a present: the probes of frames eight or more back compared draw by
    /// draw and written to `frame-N-who.txt`.
    pub(crate) fn at_present(presented: u64) {
        let Some(s) = space() else { return };
        let done: Vec<Probe> = {
            let mut p = PROBES.lock().unwrap_or_else(|e| e.into_inner());
            if p.is_empty() {
                return;
            }
            let (done, left) = std::mem::take(&mut *p).into_iter().partition(|e| e.frame + 8 <= presented);
            *p = left;
            done
        };
        if done.is_empty() {
            return;
        }
        let Some(folder) = super::folder() else { return };
        let mut text = String::new();
        // Copies are compared within one host list only: lists are recorded on
        // several threads at once, so recording order across lists is not the
        // order the card runs them in.
        let mut last: std::collections::HashMap<(usize, usize, [u32; 4]), Vec<u8>> = std::collections::HashMap::new();
        let frame = done[0].frame;
        for p in &done {
            let mut pixels = Vec::with_capacity((p.boxed[2] * p.boxed[3] * p.texel) as usize);
            for row in 0..p.boxed[3] {
                let at = s.mapped + (p.offset + u64::from(row * p.pitch)) as usize;
                // SAFETY: inside the mapped read-back buffer, written by a list that has run.
                pixels.extend_from_slice(unsafe { std::slice::from_raw_parts(at as *const u8, (p.boxed[2] * p.texel) as usize) });
            }
            let key = (p.list, p.target, p.boxed);
            match last.get(&key) {
                Some(before) => {
                    let changed = pixels.chunks(p.texel as usize).zip(before.chunks(p.texel as usize)).filter(|(a, b)| a != b).count();
                    if changed > 0 {
                        text.push_str(&format!(
                            "list {:#x} draw {} box {:?} target {:#x} changed {} of {} | {}\n",
                            p.list,
                            p.draw,
                            p.boxed,
                            p.target,
                            changed,
                            p.boxed[2] * p.boxed[3],
                            p.name
                        ));
                    }
                }
                None => text.push_str(&format!("list {:#x} draw {} box {:?} target {:#x} first in list | {}\n", p.list, p.draw, p.boxed, p.target, p.name)),
            }
            last.insert(key, pixels);
        }
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(folder.join(format!("frame-{frame}-who.txt"))) {
            let _ = f.write_all(text.as_bytes());
        }
    }
}
