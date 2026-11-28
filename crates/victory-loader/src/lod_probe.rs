//! A diagnostic (`VICTORY_LOD_PROBE=1`): what the title's per-frame batch
//! list leaves out, and why.
//!
//! What the title does (game_scarlett_master.exe 1.0.2.1, image offsets;
//! read from the disassembly, 28 Nov): +0x4e4b690 packs the list of
//! CPU-submitted batches a pass draws (`CS_PackInstances`' words
//! `(n << 29) | batch`). Its second argument (`rdx`) holds four arrays at
//! +0x50, +0x58, +0x60, +0x68, each `{ items, u16 count at +8 }`; each item
//! holds `u16` entries at +0x36 and their pointers at +8; an entry is listed
//! only when its resource (`[entry+0x40]`) is there and that resource's
//! descriptor word (`[resource+8]`) passes +0x3a990 (type 1 in bits 0-14, and
//! a size). So an entry whose resource is missing (a model not resident) is
//! left out silently.
//!
//! The packer's entry (`push r15 ; push r14 ; push r13`, 6 bytes) becomes a
//! jump to a stub that keeps `rdx`, `r8`, `r9` and a call count in cells of
//! ours, does the three pushes and jumps back. A thread of ours reads the
//! cells every 2 ms, walks the arrays with reads that cannot fault, and
//! logs every 2 s the entries seen, listed, without a resource and with a
//! descriptor that does not pass.

/// The packer's offset in the image and its first 16 bytes (the pushes and
/// `sub rsp, 68h`), compared before anything is written.
const SITE: usize = 0x4e4b690;
const BYTES: [u8; 16] = [0x41, 0x57, 0x41, 0x56, 0x41, 0x55, 0x41, 0x54, 0x56, 0x57, 0x55, 0x53, 0x48, 0x83, 0xec, 0x68];
const PATCHED: usize = 6;

/// `rdx`, `r8`, `r9` of the last call and the number of calls.
#[repr(C)]
struct Cells {
    rdx: u64,
    r8: u64,
    r9: u64,
    calls: u64,
}
static mut CELLS: Cells = Cells { rdx: 0, r8: 0, r9: 0, calls: 0 };

pub fn asked() -> bool {
    std::env::var("VICTORY_LOD_PROBE").is_ok_and(|v| v.trim() == "1")
}

fn stub(cells: u64, to: usize, back: usize) -> Option<Vec<u8>> {
    let mut code = vec![0x50]; // push rax
    code.extend([0x48, 0xb8]); // mov rax, cells
    code.extend(cells.to_le_bytes());
    code.extend([0x48, 0x89, 0x10]); // mov [rax], rdx
    code.extend([0x4c, 0x89, 0x40, 0x08]); // mov [rax+8], r8
    code.extend([0x4c, 0x89, 0x48, 0x10]); // mov [rax+16], r9
    code.extend([0xf0, 0x48, 0xff, 0x40, 0x18]); // lock inc qword [rax+24]
    code.push(0x58); // pop rax
    code.extend(&BYTES[..PATCHED]); // the three pushes
    let jump = crate::console_insn::jump(to + code.len(), back)?;
    code.extend(jump);
    Some(code)
}

/// Hooks the packer and starts the reader. Whether it did (said in the log).
///
/// # Safety
/// The image stays mapped and nothing of it runs yet.
pub unsafe fn apply(base: usize, size: usize) -> bool {
    use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
    use windows_sys::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, VirtualProtect};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    if SITE + BYTES.len() > size {
        tracing::warn!("VICTORY_LOD_PROBE: the packer is outside the image");
        return false;
    }
    let at = base + SITE;
    // SAFETY: inside the mapped image.
    if unsafe { std::slice::from_raw_parts(at as *const u8, BYTES.len()) } != BYTES {
        tracing::warn!("VICTORY_LOD_PROBE: the packer's bytes are not the ones expected (another build?): not hooked");
        return false;
    }
    let Some(place) = crate::console_insn::near(at) else {
        tracing::warn!("VICTORY_LOD_PROBE: no room for code near the title's");
        return false;
    };
    let cells = (&raw mut CELLS) as u64;
    let (Some(body), Some(jump)) = (stub(cells, place, at + PATCHED), crate::console_insn::jump(at, place)) else {
        tracing::warn!("VICTORY_LOD_PROBE: out of reach");
        return false;
    };
    let mut old = 0;
    // SAFETY: our own code memory, then the title's entry, made writable for it.
    unsafe {
        std::ptr::copy_nonoverlapping(body.as_ptr(), place as *mut u8, body.len());
        if VirtualProtect(at as *const _, PATCHED, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return false;
        }
        std::ptr::write_bytes(at as *mut u8, 0x90, PATCHED);
        std::ptr::copy_nonoverlapping(jump.as_ptr(), at as *mut u8, jump.len());
        VirtualProtect(at as *const _, PATCHED, old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), at as *const _, PATCHED);
    }
    tracing::info!("VICTORY_LOD_PROBE: the batch list packer (title+0x4e4b690) hooked");
    let _ = std::thread::Builder::new().name("lod-probe".into()).spawn(reader);
    true
}

fn read<T: Copy + Default>(at: u64) -> Option<T> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    if at < 0x10000 {
        return None;
    }
    let mut value = T::default();
    let mut got = 0usize;
    // SAFETY: a read of our own process into a local; it fails instead of faulting.
    let ok = unsafe {
        ReadProcessMemory(GetCurrentProcess(), at as *const _, (&raw mut value).cast(), size_of::<T>(), &mut got)
    };
    (ok != 0 && got == size_of::<T>()).then_some(value)
}

/// +0x3a990: nonzero when the entry is listed.
fn passes(descriptor: u64) -> bool {
    if descriptor & 0x7fff != 1 {
        return false;
    }
    let a = (descriptor >> 40) & ((1 << 23) - 1);
    let b = (descriptor >> 16) & ((1 << 23) - 1);
    (a.max(b).wrapping_sub(1).leading_zeros() as i32 - 57) != 0
}

#[derive(Default, Clone)]
struct Count {
    /// Examples: (descriptor, resource) of entries that pass and that do not.
    passing_seen: Vec<(u64, u64)>,
    failing_seen: Vec<(u64, u64)>,
    arrays: u64,
    items: u64,
    entries: u64,
    listed: u64,
    no_resource: u64,
    not_passing: u64,
    unreadable: u64,
}

fn walk(object: u64, c: &mut Count) {
    for k in 0..4u64 {
        let Some(array) = read::<u64>(object + 0x50 + 8 * k) else { c.unreadable += 1; continue };
        if array == 0 {
            continue;
        }
        c.arrays += 1;
        let (Some(items), Some(n)) = (read::<u64>(array), read::<u16>(array + 8)) else { c.unreadable += 1; continue };
        for i in 0..u64::from(n.min(20_000)) {
            let Some(item) = read::<u64>(items + 8 * i) else { c.unreadable += 1; break };
            if item == 0 {
                continue;
            }
            c.items += 1;
            let (Some(m), Some(entries)) = (read::<u16>(item + 0x36), read::<u64>(item + 8)) else { c.unreadable += 1; continue };
            for j in 0..u64::from(m.min(4096)) {
                let Some(entry) = read::<u64>(entries + 8 * j) else { c.unreadable += 1; break };
                c.entries += 1;
                match read::<u64>(entry + 0x40) {
                    None => c.unreadable += 1,
                    Some(0) => c.no_resource += 1,
                    Some(resource) => match read::<u64>(resource + 8) {
                        Some(d) if passes(d) => {
                            c.listed += 1;
                            if c.passing_seen.len() < 3 && !c.passing_seen.iter().any(|x| x.0 == d) {
                                c.passing_seen.push((d, resource));
                            }
                        }
                        Some(d) => {
                            c.not_passing += 1;
                            if c.failing_seen.len() < 6 && !c.failing_seen.iter().any(|x| x.0 == d) {
                                c.failing_seen.push((d, resource));
                            }
                        }
                        None => c.unreadable += 1,
                    },
                }
            }
        }
    }
}

fn reader() {
    let mut window = Count::default();
    let mut objects = std::collections::BTreeSet::new();
    let mut samples = 0u64;
    let mut last = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(2));
        // SAFETY: plain reads of cells the stub writes (torn values only make a walk read nothing).
        let (rdx, calls) = unsafe { (std::ptr::read_volatile(&raw const CELLS.rdx), std::ptr::read_volatile(&raw const CELLS.calls)) };
        if rdx != 0 {
            walk(rdx, &mut window);
            objects.insert(rdx);
            samples += 1;
        }
        if last.elapsed().as_secs() >= 2 {
            tracing::info!(
                calls,
                samples,
                objects = objects.len(),
                arrays = window.arrays,
                items = window.items,
                entries = window.entries,
                listed = window.listed,
                no_resource = window.no_resource,
                not_passing = window.not_passing,
                unreadable = window.unreadable,
                passing = ?window.passing_seen.iter().map(|(d, r)| format!("{d:#018x}@{r:#x}")).collect::<Vec<_>>(),
                failing = ?window.failing_seen.iter().map(|(d, r)| format!("{d:#018x}@{r:#x}")).collect::<Vec<_>>(),
                "VICTORY_LOD_PROBE: the batch list packer's entries over the last samples"
            );
            window = Count::default();
            objects.clear();
            samples = 0;
            last = std::time::Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_test_matches_the_titles() {
        assert!(!passes(0)); // type 0
        assert!(!passes(2)); // type 2
        // type 1, sizes 1024 and 16: lzcnt(1023) - 57 = -3, nonzero
        assert!(passes(1 | (16 << 16) | (1024 << 40)));
        // type 1, largest size 100: lzcnt(99) = 57: zero
        assert!(!passes(1 | (100 << 16)));
    }

    #[test]
    fn the_stub_ends_with_the_pushes_and_a_jump_back() {
        let s = stub(0x1122_3344_5566_7788, 0x1000, 0x2000).unwrap();
        let n = s.len();
        assert_eq!(&s[n - 11..n - 5], &BYTES[..6]);
        assert_eq!(s[n - 5], 0xe9);
    }
}
