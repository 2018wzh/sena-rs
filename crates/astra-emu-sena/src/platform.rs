use crate::session::Shared;
use astra_emu_abi::*;
use pal_vm::{Engine, PalWindowRequest};
use std::sync::atomic::Ordering;

pub fn service(shared: &Shared, engine: &mut Engine, next_id: &mut u64) -> anyhow::Result<()> {
    let Some(runtime) = engine.runtime_mut() else { anyhow::bail!("PAL runtime is not booted") };
    let native = runtime.system_state_mut();
    let requests = native.take_window_requests();
    for request in requests {
        anyhow::ensure!(!shared.cancelled(), "window request cancelled");
        let mut call = WindowRequest {
            session_epoch: shared.epoch, context_epoch: shared.context.load(Ordering::Acquire),
            request_id: 0, kind: 0, flags: 0, text: Bytes::EMPTY, pixels: Bytes::EMPTY,
            width: 0, height: 0, x: 0, y: 0, autohide_ms: 0,
            caption: Bytes::EMPTY, initial_text: Bytes::EMPTY,
            buttons: std::ptr::null(), button_count: 0,
        };
        let required = match request {
            PalWindowRequest::ChangeMode { mode } => {
                call.kind = WINDOW_FULLSCREEN;
                call.flags = if mode == 0 { FULLSCREEN_WINDOWED } else { FULLSCREEN_BORDERLESS };
                WINDOW_RESULT_MODE | WINDOW_RESULT_SIZE
            },
            PalWindowRequest::ChangeSize { width, height } => {
                call.kind = WINDOW_SIZE; call.width = width as u32; call.height = height as u32;
                WINDOW_RESULT_SIZE
            },
            PalWindowRequest::SetPos { x, y } => {
                call.kind = WINDOW_POSITION; call.x = x; call.y = y;
                WINDOW_RESULT_POSITION
            },
            PalWindowRequest::CursorVisibility { visible } => {
                call.kind = WINDOW_POINTER;
                call.flags = POINTER_SET_VISIBILITY | if visible { POINTER_VISIBLE } else { 0 };
                0
            },
            // Sena applies mode 3 in convert_aspect_position. There is no native
            // window ratio constraint to implement in the Manager.
            PalWindowRequest::ChangeAspect { .. } => continue,
            PalWindowRequest::CursorHideTime { time } => {
                // Upstream stores this value but implements no timer and supplies
                // no units. Preserve its native state instead of inventing a timer.
                log::warn!("Sena native cursor autohide remains unimplemented (value={time})");
                continue;
            },
        };
        *next_id = next_id.checked_add(1).ok_or_else(|| anyhow::anyhow!("window request ID exhausted"))?;
        call.request_id = *next_id;
        let service = shared.host.0.window;
        let request = service.request.ok_or_else(|| anyhow::anyhow!("native window service is unavailable"))?;
        let mut result = WindowResult { selected_button: 0, reserved: 0, valid_fields: 0, fullscreen: 0, width: 0, height: 0, x: 0, y: 0,
            input: TextBuffer { data: std::ptr::null_mut(), capacity: 0, len: 0 }, visible: 0, minimized: 0 };
        let mut error = Error::default();
        let status = unsafe { request(service.context, &call, &mut result, &mut error) };
        anyhow::ensure!(!shared.cancelled(), "window request cancelled");
        anyhow::ensure!(status == OK, "native window request {} failed ({status}): {}", call.kind,
            String::from_utf8_lossy(&error.message[..(error.message_len as usize).min(error.message.len())]));
        anyhow::ensure!(result.reserved == 0 && result.valid_fields & required == required,
            "window service omitted actual geometry");
        let (mut width, mut height) = native.window_size();
        let (mut x, mut y) = native.window_pos();
        let mut mode = native.window_mode();
        if result.valid_fields & WINDOW_RESULT_SIZE != 0 {
            anyhow::ensure!(result.width > 0 && result.height > 0 && result.width <= i32::MAX as u32 && result.height <= i32::MAX as u32,
                "window service returned invalid dimensions");
            width = result.width as i32; height = result.height as i32;
        }
        if result.valid_fields & WINDOW_RESULT_POSITION != 0 { x = result.x; y = result.y; }
        if result.valid_fields & WINDOW_RESULT_MODE != 0 {
            anyhow::ensure!(result.fullscreen <= FULLSCREEN_EXCLUSIVE, "window service returned invalid mode");
            mode = i32::from(result.fullscreen != FULLSCREEN_WINDOWED);
        }
        native.confirm_window_geometry(mode, x, y, width, height);
    }
    Ok(())
}
