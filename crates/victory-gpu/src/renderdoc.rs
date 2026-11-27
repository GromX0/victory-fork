//! RenderDoc's in-application API (diagnostics): `VICTORY_RENDERDOC=<path of
//! renderdoc.dll>` loads it before the host device is made, so it hooks
//! Direct3D 12; `VICTORY_RENDERDOC_TRIGGER=<file>`: when the file appears it
//! is removed and the next frame (present to present) is captured, into
//! `VICTORY_RENDERDOC_OUT` (a path template, `D:\victory-scratch\rdoc\cap`
//! by default). For the frame debugger's pixel history and shader stepping
//! on a frame of ours.

use std::sync::OnceLock;

/// The API's function table (`RENDERDOC_API_1_6_0`, renderdoc_app.h): the
/// slots used here.
#[repr(C)]
struct Api {
    slots: [usize; 28],
}

const SET_CAPTURE_OPTION_U32: usize = 1;
const SET_CAPTURE_FILE_PATH_TEMPLATE: usize = 11;
const GET_NUM_CAPTURES: usize = 13;
const TRIGGER_CAPTURE: usize = 15;

/// eRENDERDOC_Option_RefAllResources.
const REF_ALL_RESOURCES: u32 = 8;
/// eRENDERDOC_Option_APIValidation.
const API_VALIDATION: u32 = 2;

static API: OnceLock<Option<usize>> = OnceLock::new();

/// Loads RenderDoc if `VICTORY_RENDERDOC` names it. Call before the host
/// device is made.
pub fn load() {
    API.get_or_init(|| {
        let path = std::env::var("VICTORY_RENDERDOC").ok()?;
        use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        // SAFETY: a NUL-terminated path; RenderDoc's DLL initialises its hooks.
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            tracing::warn!(path, "VICTORY_RENDERDOC: the library did not load");
            return None;
        }
        // SAFETY: a loaded module and a NUL-terminated name.
        let get = unsafe { GetProcAddress(module, c"RENDERDOC_GetAPI".as_ptr().cast()) }?;
        type GetApi = unsafe extern "C" fn(u32, *mut *mut Api) -> i32;
        // SAFETY: RENDERDOC_GetAPI has this signature.
        let get: GetApi = unsafe { std::mem::transmute(get) };
        let mut api: *mut Api = std::ptr::null_mut();
        // SAFETY: asks for version 1.6.0 into a pointer of ours.
        if unsafe { get(10600, &mut api) } != 1 || api.is_null() {
            tracing::warn!("VICTORY_RENDERDOC: RENDERDOC_GetAPI refused version 1.6.0");
            return None;
        }
        let out = std::env::var("VICTORY_RENDERDOC_OUT").unwrap_or_else(|_| r"D:\victory-scratch\rdoc\cap".to_owned());
        let out = std::ffi::CString::new(out).ok()?;
        // SAFETY: slots of the 1.6.0 table with their signatures.
        unsafe {
            let set_u32: unsafe extern "C" fn(u32, u32) -> i32 = std::mem::transmute((*api).slots[SET_CAPTURE_OPTION_U32]);
            set_u32(REF_ALL_RESOURCES, 0);
            set_u32(API_VALIDATION, 0);
            let set_path: unsafe extern "C" fn(*const std::ffi::c_char) = std::mem::transmute((*api).slots[SET_CAPTURE_FILE_PATH_TEMPLATE]);
            set_path(out.as_ptr());
        }
        tracing::info!(path, "VICTORY_RENDERDOC: loaded");
        Some(api as usize)
    });
}

/// At a present: the trigger file, if it is there, asks for a capture of
/// the next frame.
pub(crate) fn at_present() {
    let Some(Some(api)) = API.get() else { return };
    static TRIGGER: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    let Some(file) = TRIGGER.get_or_init(|| std::env::var_os("VICTORY_RENDERDOC_TRIGGER").map(Into::into)) else { return };
    if std::fs::remove_file(file).is_err() {
        return;
    }
    let api = *api as *mut Api;
    // SAFETY: slots of the 1.6.0 table with their signatures.
    unsafe {
        let trigger: unsafe extern "C" fn() = std::mem::transmute((*api).slots[TRIGGER_CAPTURE]);
        let count: unsafe extern "C" fn() -> u32 = std::mem::transmute((*api).slots[GET_NUM_CAPTURES]);
        trigger();
        tracing::info!(captures_so_far = count(), frame = crate::queue::presented(), "VICTORY_RENDERDOC: capture of the next frame asked");
    }
}
