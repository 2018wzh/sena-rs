//! C ABI surface used by the platform launcher projects.
//!
//! The desktop entry point delegates to the same `run_sena` path as the
//! command-line binary. Mobile host-mode entry points are exported so the
//! launcher projects link cleanly, but they report unsupported state until a
//! real mobile renderer/event-pump implementation is connected.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::{Path, PathBuf};
use std::ptr;
use std::str::FromStr;
use std::time::Duration;

use pal_asset::Nls;

use crate::app::build_engine;
use crate::config::TextLang;
use crate::engine::Engine;
use crate::event::{InputEvent, MouseButton, PalEvent};
use crate::runtime::RuntimeStatus;
use crate::scene::{rasterize_scene_rgba, FrameScene};

type NativeMessageboxCallback =
    Option<extern "C" fn(*mut c_void, u64, i32, *const c_char, *const c_char)>;

pub struct SenaFfiHost {
    engine: Engine,
    rgba: Vec<u8>,
    frame_width: u32,
    frame_height: u32,
    frame_generation: u64,
    last_status: i32,
}

fn cstring_lossy(s: impl AsRef<str>) -> *mut c_char {
    let clean = s.as_ref().replace('\0', " ");
    CString::new(clean)
        .unwrap_or_else(|_| CString::new("Sena").expect("static fallback is a valid CString"))
        .into_raw()
}

unsafe fn path_from_cstr(ptr: *const c_char) -> Result<PathBuf, &'static str> {
    if ptr.is_null() {
        return Err("null path");
    }
    let s = CStr::from_ptr(ptr)
        .to_str()
        .map_err(|_| "path is not UTF-8")?;
    if s.trim().is_empty() {
        return Err("empty path");
    }
    Ok(PathBuf::from(s))
}

unsafe fn string_from_cstr(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok().map(str::to_owned)
}

fn nls_from_cstr(ptr: *const c_char) -> Nls {
    unsafe { string_from_cstr(ptr) }
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .and_then(|s| Nls::from_str(s).ok())
        .unwrap_or_default()
}

fn lang_from_cstr(ptr: *const c_char) -> TextLang {
    unsafe { string_from_cstr(ptr) }
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .and_then(|s| TextLang::from_str(s).ok())
        .unwrap_or(TextLang::Jp)
}

fn runtime_status_is_terminal(status: &RuntimeStatus) -> bool {
    matches!(
        status,
        RuntimeStatus::Halted { .. }
            | RuntimeStatus::UnsupportedCommand { .. }
            | RuntimeStatus::UnsupportedExtCall { .. }
            | RuntimeStatus::Faulted { .. }
    )
}

fn host_from_handle<'a>(handle: *mut c_void) -> Option<&'a mut SenaFfiHost> {
    if handle.is_null() {
        None
    } else {
        unsafe { (handle as *mut SenaFfiHost).as_mut() }
    }
}

fn cover_candidate(root: &Path) -> Option<PathBuf> {
    const NAMES: &[&str] = &[
        "cover.png",
        "cover.jpg",
        "cover.jpeg",
        "Cover.png",
        "Cover.jpg",
        "icon.png",
        "Icon.png",
    ];
    NAMES
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
}

fn mime_for_path(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("png") => Some("image/png"),
        Some(ext) if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") => {
            Some("image/jpeg")
        }
        _ => None,
    }
}

/// Releases strings returned by the Sena C ABI.
#[no_mangle]
pub extern "C" fn sena_free_c_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr));
        }
    }
}

/// Alias retained for the platform launcher headers.
#[no_mangle]
pub extern "C" fn sena_string_free(ptr: *mut c_char) {
    sena_free_c_string(ptr);
}

/// Returns a display name for a game directory.
#[no_mangle]
pub unsafe extern "C" fn sena_game_name_from_dir(game_root_utf8: *const c_char) -> *mut c_char {
    match path_from_cstr(game_root_utf8) {
        Ok(path) => {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or("Sena");
            cstring_lossy(name)
        }
        Err(_) => cstring_lossy("Sena"),
    }
}

/// Returns a best-effort cover image path for launcher UI previews.
#[no_mangle]
pub unsafe extern "C" fn sena_game_cover_path_from_dir(
    game_root_utf8: *const c_char,
) -> *mut c_char {
    match path_from_cstr(game_root_utf8)
        .ok()
        .and_then(|root| cover_candidate(&root))
    {
        Some(path) => cstring_lossy(path.to_string_lossy()),
        None => ptr::null_mut(),
    }
}

/// Returns the MIME type for the cover image discovered by `sena_game_cover_path_from_dir`.
#[no_mangle]
pub unsafe extern "C" fn sena_game_cover_mime_from_dir(
    game_root_utf8: *const c_char,
) -> *mut c_char {
    match path_from_cstr(game_root_utf8)
        .ok()
        .and_then(|root| cover_candidate(&root))
        .and_then(|path| mime_for_path(&path).map(str::to_owned))
    {
        Some(mime) => cstring_lossy(mime),
        None => ptr::null_mut(),
    }
}

/// Runs the desktop Sena engine with `game_root_utf8` as the game directory.
#[no_mangle]
pub unsafe extern "C" fn sena_run_entry(game_root_utf8: *const c_char) -> i32 {
    sena_run_entry_nls(game_root_utf8, ptr::null(), ptr::null())
}

/// Runs the desktop Sena engine with an explicit text/resource encoding.
#[no_mangle]
pub unsafe extern "C" fn sena_run_entry_nls(
    game_root_utf8: *const c_char,
    nls_utf8: *const c_char,
    lang_utf8: *const c_char,
) -> i32 {
    let game_root = match path_from_cstr(game_root_utf8) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("sena_run_entry: {err}");
            return 2;
        }
    };

    match crate::run_sena(crate::SenaConfig {
        game_root: Some(game_root),
        nls: nls_from_cstr(nls_utf8),
        lang: lang_from_cstr(lang_utf8),
        ..Default::default()
    }) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("sena_run_entry failed: {err:?}");
            1
        }
    }
}

/// Creates a platform-owned PAL host that renders each frame into an RGBA buffer.
#[no_mangle]
pub unsafe extern "C" fn sena_host_create(
    game_root_utf8: *const c_char,
    nls_utf8: *const c_char,
    lang_utf8: *const c_char,
    width: u32,
    height: u32,
) -> *mut c_void {
    let game_root = match path_from_cstr(game_root_utf8) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("sena_host_create: {err}");
            return ptr::null_mut();
        }
    };
    let mut config = crate::SenaConfig {
        game_root: Some(game_root),
        nls: nls_from_cstr(nls_utf8),
        lang: lang_from_cstr(lang_utf8),
        width: width.max(1),
        height: height.max(1),
        prefer_config_window_size: true,
        ..Default::default()
    };
    config.renderer.virtual_width = width.max(1);
    config.renderer.virtual_height = height.max(1);

    let mut engine = match build_engine(&config) {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("sena_host_create failed: {err:?}");
            return ptr::null_mut();
        }
    };
    let (surface_width, surface_height) =
        engine.window_size_from_config(width.max(1), height.max(1));
    engine.handle_event(PalEvent::Resized {
        width: surface_width,
        height: surface_height,
    });

    Box::into_raw(Box::new(SenaFfiHost {
        engine,
        rgba: Vec::new(),
        frame_width: surface_width.max(1),
        frame_height: surface_height.max(1),
        frame_generation: 0,
        last_status: 0,
    })) as *mut c_void
}

/// Advances the host by one platform frame and refreshes the RGBA framebuffer.
#[no_mangle]
pub extern "C" fn sena_host_step(handle: *mut c_void, dt_ms: u32) -> i32 {
    let Some(host) = host_from_handle(handle) else {
        return 2;
    };
    host.engine.handle_event(PalEvent::RedrawRequested);
    let frame = match host
        .engine
        .update_with_delta(Duration::from_millis(dt_ms.max(1).min(250) as u64))
    {
        Ok(frame) => frame,
        Err(err) => {
            eprintln!("sena_host_step failed: {err:?}");
            host.last_status = 2;
            host.engine.input_begin_frame();
            return 2;
        }
    };
    host.frame_width = frame.scene.logical_width.max(1);
    host.frame_height = frame.scene.logical_height.max(1);
    host.rgba = rasterize_scene_rgba(&frame.scene);
    host.frame_generation = host.frame_generation.wrapping_add(1);
    let terminal = runtime_status_is_terminal(&frame.runtime_status);
    host.engine.input_begin_frame();
    host.last_status = if terminal { 1 } else { 0 };
    host.last_status
}

#[no_mangle]
pub extern "C" fn sena_host_resize(handle: *mut c_void, width: u32, height: u32) {
    if let Some(host) = host_from_handle(handle) {
        host.engine.handle_event(PalEvent::Resized {
            width: width.max(1),
            height: height.max(1),
        });
    }
}

#[no_mangle]
pub extern "C" fn sena_host_touch(handle: *mut c_void, phase: i32, x: f64, y: f64) {
    let Some(host) = host_from_handle(handle) else {
        return;
    };
    host.engine
        .handle_event(PalEvent::Input(InputEvent::CursorMoved { x, y }));
    match phase {
        0 => host
            .engine
            .handle_event(PalEvent::Input(InputEvent::MouseInput {
                button: MouseButton::Left,
                pressed: true,
            })),
        2 | 3 => host
            .engine
            .handle_event(PalEvent::Input(InputEvent::MouseInput {
                button: MouseButton::Left,
                pressed: false,
            })),
        _ => {}
    }
}

#[no_mangle]
pub unsafe extern "C" fn sena_host_key(handle: *mut c_void, key_utf8: *const c_char, pressed: i32) {
    let Some(host) = host_from_handle(handle) else {
        return;
    };
    let Some(key_name) = string_from_cstr(key_utf8) else {
        return;
    };
    host.engine
        .handle_event(PalEvent::Input(InputEvent::Keyboard {
            key_name,
            pressed: pressed != 0,
        }));
}

#[no_mangle]
pub extern "C" fn sena_host_frame_rgba(handle: *mut c_void) -> *const u8 {
    host_from_handle(handle)
        .map(|host| host.rgba.as_ptr())
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub extern "C" fn sena_host_frame_width(handle: *mut c_void) -> u32 {
    host_from_handle(handle)
        .map(|host| host.frame_width)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn sena_host_frame_height(handle: *mut c_void) -> u32 {
    host_from_handle(handle)
        .map(|host| host.frame_height)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn sena_host_frame_generation(handle: *mut c_void) -> u64 {
    host_from_handle(handle)
        .map(|host| host.frame_generation)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn sena_host_destroy(handle: *mut c_void) {
    if !handle.is_null() {
        unsafe {
            drop(Box::from_raw(handle as *mut SenaFfiHost));
        }
    }
}

/// The native-activity entry point the Android platform never calls.
///
/// `winit`'s `android-native-activity` backend links the native-activity glue,
/// and the glue is what the application library is supposed to complete: the
/// entry point belongs to the app, not to the glue. This platform drives the
/// engine from Java through JNI and a `SurfaceView`
/// (`platform/android/app/src/main/cpp/sena_jni.cpp`), so nothing ever calls
/// it -- but the symbol has to exist, because an undefined one makes the
/// dynamic linker refuse to load the library at all
/// (`dlopen failed: cannot locate symbol "android_main"`), which is what the
/// launcher saw when it asked the engine for a game's title.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn android_main(_app: *mut c_void) {
    log::warn!(
        "android_main was called, but the Android host drives the engine through JNI"
    );
}

#[no_mangle]
pub extern "C" fn sena_android_init_context(_java_vm_ptr: *mut c_void, _context_ptr: *mut c_void) {}

#[no_mangle]
pub unsafe extern "C" fn sena_android_create(
    _native_window_ptr: *mut c_void,
    surface_width_px: u32,
    surface_height_px: u32,
    _native_scale_factor: f64,
    game_dir_utf8: *const c_char,
    nls_utf8: *const c_char,
    lang_utf8: *const c_char,
) -> *mut c_void {
    sena_host_create(
        game_dir_utf8,
        nls_utf8,
        lang_utf8,
        surface_width_px,
        surface_height_px,
    )
}

#[no_mangle]
pub extern "C" fn sena_android_set_native_messagebox_callback(
    _handle: *mut c_void,
    _callback: NativeMessageboxCallback,
    _user_data: *mut c_void,
) {
}

#[no_mangle]
pub extern "C" fn sena_android_submit_messagebox_result(
    _handle: *mut c_void,
    _request_id: u64,
    _value: i64,
) {
}

#[no_mangle]
pub extern "C" fn sena_android_step(handle: *mut c_void, dt_ms: u32) -> i32 {
    sena_host_step(handle, dt_ms)
}

#[no_mangle]
pub extern "C" fn sena_android_resize(
    handle: *mut c_void,
    surface_width_px: u32,
    surface_height_px: u32,
) {
    sena_host_resize(handle, surface_width_px, surface_height_px);
}

#[no_mangle]
pub extern "C" fn sena_android_set_surface(
    handle: *mut c_void,
    _native_window_ptr: *mut c_void,
    surface_width_px: u32,
    surface_height_px: u32,
) {
    sena_host_resize(handle, surface_width_px, surface_height_px);
}

#[no_mangle]
pub extern "C" fn sena_android_touch(handle: *mut c_void, phase: i32, x_px: f64, y_px: f64) {
    sena_host_touch(handle, phase, x_px, y_px);
}

#[no_mangle]
pub extern "C" fn sena_android_destroy(handle: *mut c_void) {
    sena_host_destroy(handle);
}

#[no_mangle]
pub extern "C" fn sena_android_frame_rgba(handle: *mut c_void) -> *const u8 {
    sena_host_frame_rgba(handle)
}

#[no_mangle]
pub extern "C" fn sena_android_frame_width(handle: *mut c_void) -> u32 {
    sena_host_frame_width(handle)
}

#[no_mangle]
pub extern "C" fn sena_android_frame_height(handle: *mut c_void) -> u32 {
    sena_host_frame_height(handle)
}

#[no_mangle]
pub extern "C" fn sena_android_frame_generation(handle: *mut c_void) -> u64 {
    sena_host_frame_generation(handle)
}

#[no_mangle]
pub unsafe extern "C" fn sena_ios_create(
    _ui_view: *mut c_void,
    surface_width: u32,
    surface_height: u32,
    _native_scale_factor: f64,
    game_root_utf8: *const c_char,
    nls_utf8: *const c_char,
    lang_utf8: *const c_char,
) -> *mut c_void {
    sena_host_create(
        game_root_utf8,
        nls_utf8,
        lang_utf8,
        surface_width,
        surface_height,
    )
}

#[no_mangle]
pub extern "C" fn sena_ios_resize_viewport(
    handle: *mut c_void,
    _surface_width: u32,
    _surface_height: u32,
    _viewport_x: u32,
    _viewport_y: u32,
    viewport_width: u32,
    viewport_height: u32,
) {
    sena_host_resize(handle, viewport_width.max(1), viewport_height.max(1));
}

#[no_mangle]
pub unsafe extern "C" fn sena_ios_logical_size(
    _handle: *mut c_void,
    width_out: *mut u32,
    height_out: *mut u32,
) {
    let (w, h) = host_from_handle(_handle)
        .map(|host| {
            host.engine.logical_size_from_config(
                FrameScene::PAL_DEFAULT_WIDTH,
                FrameScene::PAL_DEFAULT_HEIGHT,
            )
        })
        .unwrap_or((1280, 720));
    if !width_out.is_null() {
        *width_out = w;
    }
    if !height_out.is_null() {
        *height_out = h;
    }
}

#[no_mangle]
pub extern "C" fn sena_ios_set_native_messagebox_callback(
    _handle: *mut c_void,
    _callback: NativeMessageboxCallback,
    _user_data: *mut c_void,
) {
}

#[no_mangle]
pub extern "C" fn sena_ios_submit_messagebox_result(
    _handle: *mut c_void,
    _request_id: u64,
    _value: i64,
) {
}

#[no_mangle]
pub extern "C" fn sena_ios_step(handle: *mut c_void, dt_ms: u32) -> i32 {
    sena_host_step(handle, dt_ms)
}

#[no_mangle]
pub extern "C" fn sena_ios_resize(handle: *mut c_void, surface_width: u32, surface_height: u32) {
    sena_host_resize(handle, surface_width, surface_height);
}

#[no_mangle]
pub extern "C" fn sena_ios_touch(handle: *mut c_void, phase: i32, x_points: f64, y_points: f64) {
    sena_host_touch(handle, phase, x_points, y_points);
}

#[no_mangle]
pub extern "C" fn sena_ios_destroy(handle: *mut c_void) {
    sena_host_destroy(handle);
}

#[no_mangle]
pub extern "C" fn sena_ios_frame_rgba(handle: *mut c_void) -> *const u8 {
    sena_host_frame_rgba(handle)
}

#[no_mangle]
pub extern "C" fn sena_ios_frame_width(handle: *mut c_void) -> u32 {
    sena_host_frame_width(handle)
}

#[no_mangle]
pub extern "C" fn sena_ios_frame_height(handle: *mut c_void) -> u32 {
    sena_host_frame_height(handle)
}

#[no_mangle]
pub extern "C" fn sena_ios_frame_generation(handle: *mut c_void) -> u64 {
    sena_host_frame_generation(handle)
}

#[repr(C)]
pub struct SenaPumpHandle {
    _private: [u8; 0],
}

#[no_mangle]
pub unsafe extern "C" fn sena_pump_create(game_root_utf8: *const c_char) -> *mut SenaPumpHandle {
    sena_pump_create_nls(game_root_utf8, ptr::null(), ptr::null())
}

#[no_mangle]
pub unsafe extern "C" fn sena_pump_create_nls(
    game_root_utf8: *const c_char,
    nls_utf8: *const c_char,
    lang_utf8: *const c_char,
) -> *mut SenaPumpHandle {
    sena_host_create(game_root_utf8, nls_utf8, lang_utf8, 1280, 720) as *mut SenaPumpHandle
}

#[no_mangle]
pub extern "C" fn sena_pump_set_native_messagebox_callback(
    _handle: *mut SenaPumpHandle,
    _callback: NativeMessageboxCallback,
    _user_data: *mut c_void,
) {
}

#[no_mangle]
pub extern "C" fn sena_pump_submit_messagebox_result(
    _handle: *mut SenaPumpHandle,
    _request_id: u64,
    _value: i64,
) {
}

#[no_mangle]
pub extern "C" fn sena_pump_step(_handle: *mut SenaPumpHandle, _timeout_ms: u32) -> i32 {
    sena_host_step(_handle as *mut c_void, _timeout_ms.max(1))
}

#[no_mangle]
pub extern "C" fn sena_pump_destroy(_handle: *mut SenaPumpHandle) {
    sena_host_destroy(_handle as *mut c_void);
}
