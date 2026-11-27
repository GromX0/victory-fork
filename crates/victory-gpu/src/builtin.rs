//! Built-in changes to the title's console shader code (`patches/builtin.txt`):
//! a few instruction words replaced before translation, each only where the
//! title's code holds the word recorded beside it. Translated through the
//! shader cache (the change is part of the entry's key).
//!
//! Groups:
//! - `dissolve` (on; `VICTORY_DISSOLVE=0`: off): the object pixel shaders'
//!   dissolve dither kills no pixel. Objects the title hands a part-way
//!   dissolve were drawn with most of their pixels discarded here: fences,
//!   bushes and bins ghosting or gone between near and far (27 Nov, who4
//!   captures; objfix: solid, water untouched). 1,301 sites of the
//!   `v_cmpx_gt_f32` after the interleaved-gradient noise (0x4253ee82) in
//!   `PS_Deferred*`, `PS_PropFoliageDeferred*` and their mirror variants;
//!   water, effects, depth, alpha and glass shaders are left as they are.
//!   shortcut: the dissolve values themselves are not corrected; detail
//!   levels switch without the dithered crossfade.
//! - `occluders` (on; `VICTORY_CPU_OCCLUDERS=1`: the title's): the CPU's
//!   occluder samples left out of the depth pyramid (`42683eeb`); they reach
//!   the card late here and hid objects in view (27 Nov, usernocpu).

use std::collections::HashMap;
use std::sync::OnceLock;

/// (byte offset in the machine code, the title's dword, ours).
type Site = (usize, u32, u32);

fn on(group: &str) -> bool {
    let var = |name: &str| std::env::var(name).ok().map(|v| v.trim().to_owned());
    match group {
        "dissolve" => var("VICTORY_DISSOLVE").as_deref() != Some("0"),
        "occluders" => var("VICTORY_CPU_OCCLUDERS").as_deref() != Some("1"),
        _ => false,
    }
}

fn table() -> &'static HashMap<String, Vec<Site>> {
    static TABLE: OnceLock<HashMap<String, Vec<Site>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table: HashMap<String, Vec<Site>> = HashMap::new();
        let mut groups: HashMap<&str, usize> = HashMap::new();
        for line in include_str!("patches/builtin.txt").lines() {
            let words: Vec<&str> = line.split('#').next().unwrap_or("").split_whitespace().collect();
            let [group, hash, at, title, ours] = words[..] else { continue };
            if !on(group) {
                continue;
            }
            let hex = |s: &str| u32::from_str_radix(s, 16).ok();
            let (Ok(at), Some(title), Some(ours)) = (usize::from_str_radix(at, 16), hex(title), hex(ours)) else {
                continue;
            };
            table.entry(hash.to_ascii_lowercase()).or_default().push((at, title, ours));
            *groups.entry(group).or_default() += 1;
        }
        tracing::info!(?groups, "built-in shader changes (VICTORY_DISSOLVE=0, VICTORY_CPU_OCCLUDERS=1: off)");
        table
    })
}

/// Whether shader `hash` has built-in changes.
pub(crate) fn has(hash: &str) -> bool {
    hash.get(..16).is_some_and(|h| table().contains_key(h))
}

/// The built-in changes for `container` (shader `hash`) as (offset, dword),
/// when its machine code holds the title's dwords at every site; `None`
/// otherwise (logged).
#[cfg(windows)]
pub(crate) fn patch(container: &[u8], hash: &str) -> Option<Vec<(usize, u32)>> {
    use victory_shader::{Container, FourCC, HwStage};
    let sites = table().get(hash.get(..16)?)?;
    let c = Container::parse(container).ok()?;
    let part = c.part(FourCC::DXIL).or(c.part(FourCC::ACCC))?;
    let programs = victory_shader::native_programs(part.data).ok()?;
    let program = programs
        .iter()
        .find(|p| matches!(p.stage, HwStage::Vs | HwStage::Ps | HwStage::Ms | HwStage::Cs))?;
    let word = |at: usize| program.code.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if let Some(&(at, title, _)) = sites.iter().find(|&&(at, title, _)| word(at) != Some(title)) {
        tracing::warn!(hash, at = format!("{at:#x}"), title = format!("{title:08x}"), found = format!("{:08x?}", word(at)), "built-in shader change: the code is not the expected one; left as it is");
        return None;
    }
    Some(sites.iter().map(|&(at, _, ours)| (at, ours)).collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_table_parses_and_every_site_is_whole() {
        let lines = include_str!("patches/builtin.txt").lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty());
        let mut n = 0;
        for line in lines {
            let w: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(w.len(), 5, "{line}");
            assert!(matches!(w[0], "dissolve" | "occluders"), "{line}");
            assert_eq!(w[1].len(), 16, "{line}");
            assert!(usize::from_str_radix(w[2], 16).is_ok_and(|a| a % 4 == 0), "{line}");
            assert!(u32::from_str_radix(w[3], 16).is_ok() && u32::from_str_radix(w[4], 16).is_ok(), "{line}");
            n += 1;
        }
        assert_eq!(n, 1310);
    }
}
