//! The title's clocks (`QueryPerformanceCounter`, the C++ runtime's
//! `_Query_perf_counter`, `GetTickCount64`), optionally slowed: with
//! `VICTORY_CLOCK_SCALE=<s>` (0 < s <= 1) the title sees `s` seconds pass for
//! each real one, counted from its first call. A diagnostic: a title that
//! adapts its quality to its own frame time sees the frame time it would at
//! `1 / s` times our frame rate (its own 30 fps limiter, which waits on the
//! same counter, then holds it at 30 as on the console); its world runs `s`
//! times as fast. Unset: the host's counters, unchanged.

#![allow(non_snake_case)]

use std::sync::OnceLock;

use crate::{HleModule, hle_export};

fn scale() -> Option<f64> {
    static SCALE: OnceLock<Option<f64>> = OnceLock::new();
    *SCALE.get_or_init(|| {
        let s = std::env::var("VICTORY_CLOCK_SCALE").ok()?.trim().parse::<f64>().ok()?;
        (s > 0.0 && s <= 1.0 && s != 1.0).then(|| {
            tracing::info!(scale = s, "VICTORY_CLOCK_SCALE: the title's clocks run slower than real time");
            s
        })
    })
}

fn host_counter() -> i64 {
    let mut count = 0i64;
    // SAFETY: a valid out pointer.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut count) };
    count
}

/// `raw` (a reading of a clock that started at `base`) as the title sees it.
fn scaled(raw: i64, base: i64, s: f64) -> i64 {
    base + ((raw - base) as f64 * s) as i64
}

fn counter() -> i64 {
    let raw = host_counter();
    let Some(s) = scale() else { return raw };
    static BASE: OnceLock<i64> = OnceLock::new();
    scaled(raw, *BASE.get_or_init(|| raw), s)
}

pub extern "system" fn QueryPerformanceCounter(out: *mut i64) -> i32 {
    if out.is_null() {
        return 0;
    }
    // SAFETY: checked non-null; the title passes room for the count.
    unsafe { out.write(counter()) };
    1
}

pub extern "C" fn _Query_perf_counter() -> i64 {
    counter()
}

pub extern "system" fn GetTickCount64() -> u64 {
    // SAFETY: no arguments.
    let raw = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    let Some(s) = scale() else { return raw };
    static BASE: OnceLock<u64> = OnceLock::new();
    scaled(raw as i64, *BASE.get_or_init(|| raw) as i64, s) as u64
}

pub(crate) fn module() -> HleModule {
    HleModule {
        name: "clock",
        dlls: &["api-ms-win-core-profile-*", "msvcp140.dll", "api-ms-win-core-sysinfo-*"],
        exports: vec![hle_export!(QueryPerformanceCounter), hle_export!(_Query_perf_counter), hle_export!(GetTickCount64)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scaled_clock_runs_from_its_base_at_the_scale() {
        assert_eq!(scaled(1_000, 1_000, 0.5), 1_000);
        assert_eq!(scaled(3_000, 1_000, 0.5), 2_000);
        assert_eq!(scaled(1_000_000, 0, 0.25), 250_000);
    }

    #[test]
    fn unset_it_is_the_host_counter() {
        let a = host_counter();
        let b = counter();
        assert!(b >= a);
    }
}
