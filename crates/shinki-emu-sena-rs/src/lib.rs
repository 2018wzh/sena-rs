//! Koikake's native PAL VM, GPU compositor, Kira audio and saves behind one C ABI.
mod config;
mod platform;
mod session;

use shinki_emu_abi::*;
use config::text;
use pal_vm::{InputEvent as NativeInput, MouseButton, PalEvent};
use session::{Command, SessionOwner};
use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

static INITIALIZED: AtomicBool = AtomicBool::new(false);
const API: CoreApi = CoreApi {
    header: Header::of::<CoreApi>(),
    initialize: Some(initialize),
    describe: Some(describe),
    probe: Some(probe),
    open: Some(open),
    input: Some(input),
    control: Some(control),
    state: Some(state),
    frame: Some(frame),
    cancel: Some(cancel),
    close: Some(close),
    extension: Some(extension),
};
struct Formats([Bytes; 1]);
unsafe impl Sync for Formats {}
static FORMATS: Formats = Formats([text("softpal-koikake")]);

/// Static registration uses this same function. Query never creates native state.
///
/// # Safety
/// A non-null, aligned `output` must point to writable storage for one API pointer.
#[cfg_attr(feature = "diagnostic-bridge", no_mangle)]
pub unsafe extern "C" fn shinki_emu_query(version: u32, output: *mut *const CoreApi) -> Status {
    if output.is_null() || !output.is_aligned() {
        return INVALID_ARGUMENT;
    }
    output.write(std::ptr::null());
    if version != ABI_VERSION {
        return UNSUPPORTED;
    }
    output.write(&API);
    OK
}
pub const QUERY: Query = shinki_emu_query;
type Failure = (Status, String);
fn fail(status: Status, message: impl Into<String>) -> Failure {
    (status, message.into())
}
fn check(status: Status) -> Failure {
    fail(status, "invalid ABI input")
}
unsafe fn boundary(error: *mut Error, operation: impl FnOnce() -> Result<(), Failure>) -> Status {
    if let Some(error) = error.as_mut() {
        *error = Error::default();
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(Ok(())) => OK,
        Ok(Err((status, message))) => {
            if let Some(error) = error.as_mut() {
                error.set("sena.abi", &message);
            }
            status
        }
        Err(_) => {
            if let Some(error) = error.as_mut() {
                error.set("sena.panic", "adapter panicked");
            }
            PANICKED
        }
    }
}
unsafe fn session_boundary(
    handle: Session,
    error: *mut Error,
    operation: impl FnOnce() -> Result<(), Failure>,
) -> Status {
    let status = boundary(error, operation);
    if status == PANICKED {
        if let Ok(owner) = owner(handle) {
            owner
                .shared
                .fail("sena.panic", "adapter panic poisoned the session");
            owner.shared.cancel();
        }
    }
    status
}
unsafe fn reference<'a, T>(pointer: *const T) -> Result<&'a T, Failure> {
    if pointer.is_null() || !pointer.is_aligned() {
        Err(check(INVALID_ARGUMENT))
    } else {
        Ok(&*pointer)
    }
}
unsafe fn destination<'a, T>(pointer: *mut T) -> Result<&'a mut T, Failure> {
    if pointer.is_null() || !pointer.is_aligned() {
        Err(check(INVALID_ARGUMENT))
    } else {
        Ok(&mut *pointer)
    }
}
unsafe fn owner<'a>(handle: Session) -> Result<&'a SessionOwner, Failure> {
    reference(handle.cast())
}
fn initialized() -> Result<(), Failure> {
    if INITIALIZED.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err(fail(INVALID_STATE, "initialize diagnostics first"))
    }
}
unsafe extern "C" fn initialize(diagnostics: *const Diagnostics, error: *mut Error) -> Status {
    boundary(error, || {
        let diagnostics = reference(diagnostics)?;
        if !diagnostics.header.accepts::<Diagnostics>()
            || diagnostics.emit.is_none()
            || diagnostics.flush.is_none()
        {
            return Err(check(INVALID_ARGUMENT));
        }
        #[cfg(feature = "diagnostic-bridge")]
        diagnostic_bridge::install(*diagnostics)
            .map_err(|status| fail(status, "diagnostic bridge installation failed"))?;
        INITIALIZED.store(true, Ordering::Release);
        Ok(())
    })
}
unsafe extern "C" fn describe(output: *mut Descriptor, error: *mut Error) -> Status {
    boundary(error, || {
        initialized()?;
        *destination(output)? = Descriptor {
            header: Header::of::<Descriptor>(),
            family_id: text("softpal"),
            plugin_id: text("sena-rs"),
            version: text(env!("CARGO_PKG_VERSION")),
            capabilities: CAP_CPU_SURFACE | CAP_RENDER_EXTENT,
            formats: FORMATS.0.as_ptr(),
            format_count: FORMATS.0.len(),
            configuration: config::SCHEMA.0.as_ptr(),
            configuration_count: config::SCHEMA.0.len(),
            display_name: text("Sena"),
            family_name: text("SoftPAL"),
            render: config::RENDER,
        };
        Ok(())
    })
}
unsafe extern "C" fn probe(path: Bytes, output: *mut ProbeResult, error: *mut Error) -> Status {
    boundary(error, || {
        initialized()?;
        let output = destination(output)?;
        *output = ProbeResult {
            game_id: Bytes::EMPTY,
            format: Bytes::EMPTY,
            confidence_permyriad: 0,
            reserved: 0,
        };
        let root = config::game_root(path.text(32768).map_err(check)?);
        if !config::matches_game(&root) {
            return Err(fail(
                NO_MATCH,
                "Koikake executable or Steam identity not found",
            ));
        }
        *output = ProbeResult {
            game_id: text("koikake"),
            format: text("softpal-koikake"),
            confidence_permyriad: 8000,
            reserved: 0,
        };
        Ok(())
    })
}
fn valid_window(window: WindowState) -> bool {
    window.scale_factor.is_finite()
        && window.scale_factor > 0.0
        && window.focused <= 1
        && window.suspended <= 1
}
unsafe extern "C" fn open(
    request: *const OpenRequest,
    output: *mut OpenResult,
    error: *mut Error,
) -> Status {
    boundary(error, || {
        let output = destination(output)?;
        // A failed open never leaves a live handle, even when validation fails.
        output.session = std::ptr::null_mut();
        initialized()?;
        let request = reference(request)?;
        if !request.header.accepts::<OpenRequest>()
            || !request.host.header.accepts::<HostServices>()
            || request.session_epoch == 0
            || !valid_window(request.window)
            || request.host.cancellation.is_cancelled.is_none()
            || (request.host.window.request.is_some() && request.host.window.invalidate.is_none())
        {
            return Err(check(INVALID_ARGUMENT));
        }
        if request.host.window.request.is_none() {
            return Err(fail(
                UNSUPPORTED,
                "Sena Embedded requires a native window host",
            ));
        }
        let settings = config::Settings::read(request).map_err(check)?;
        if !settings.root.is_dir() {
            return Err(fail(
                INVALID_ARGUMENT,
                "game root must be an existing directory",
            ));
        }
        let owner = Box::new(SessionOwner::open(settings, request)?);
        if owner.shared.cancelled() {
            return Err(fail(CANCELLED, "session open cancelled"));
        }
        output.context_epoch = 1;
        output.output_epoch = 1;
        output.surface = owner.shared.lock().info;
        owner.start();
        output.session = Box::into_raw(owner).cast();
        Ok(())
    })
}
unsafe extern "C" fn input(
    handle: Session,
    events: *const InputEvent,
    count: usize,
    error: *mut Error,
) -> Status {
    session_boundary(handle, error, || {
        let owner = owner(handle)?;
        if owner.shared.cancelled() {
            return Err(fail(CANCELLED, "session cancelled"));
        }
        let events = borrowed_array(events, count, 1024).map_err(check)?;
        let mut state = owner.shared.lock();
        if matches!(state.status, SESSION_FAILED | SESSION_FINISHED) {
            return Err(check(INVALID_STATE));
        }
        let context = owner.shared.context.load(Ordering::Acquire);
        let mut commands = Vec::with_capacity(events.len());
        for event in events {
            if event.pressed > 1 || event.repeat > 1 {
                return Err(check(INVALID_ARGUMENT));
            }
            if matches!(event.kind, INPUT_POINTER_MOVE | INPUT_POINTER_BUTTON) {
                if event.displayed.session_epoch != owner.shared.epoch
                    || event.displayed.output_epoch != context
                    || event.displayed.generation == 0
                    || state
                        .frame
                        .as_ref()
                        .is_none_or(|frame| event.displayed.generation > frame.identity.generation)
                {
                    return Err(check(STALE_CONTEXT));
                }
                if !event.x.is_finite() || !event.y.is_finite() {
                    return Err(check(INVALID_ARGUMENT));
                }
            }
            let event_native = match event.kind {
                INPUT_KEY => {
                    let logical = event.logical_key.text(256).map_err(check)?;
                    let physical = event.physical_key.text(256).map_err(check)?;
                    Some(NativeInput::Keyboard {
                        key_name: if logical.is_empty() {
                            physical
                        } else {
                            logical
                        }
                        .to_owned(),
                        pressed: event.pressed != 0,
                    })
                }
                INPUT_POINTER_MOVE => Some(NativeInput::CursorMoved {
                    x: event.x,
                    y: event.y,
                }),
                INPUT_POINTER_BUTTON => {
                    let button = match event.button {
                        BUTTON_PRIMARY => MouseButton::Left,
                        BUTTON_SECONDARY => MouseButton::Right,
                        BUTTON_MIDDLE => MouseButton::Middle,
                        _ => return Err(check(INVALID_ARGUMENT)),
                    };
                    commands.push(Command::Event(
                        PalEvent::Input(NativeInput::CursorMoved {
                            x: event.x,
                            y: event.y,
                        }),
                        context,
                    ));
                    Some(NativeInput::MouseInput {
                        button,
                        pressed: event.pressed != 0,
                    })
                }
                INPUT_WHEEL => {
                    if !event.x.is_finite()
                        || !event.y.is_finite()
                        || event.x.abs() > f32::MAX as f64
                        || event.y.abs() > f32::MAX as f64
                    {
                        return Err(check(INVALID_ARGUMENT));
                    }
                    Some(NativeInput::MouseWheel {
                        delta_x: event.x as f32,
                        delta_y: event.y as f32,
                    })
                }
                INPUT_WINDOW => {
                    if !valid_window(event.window) {
                        return Err(check(INVALID_ARGUMENT));
                    }
                    commands.push(Command::Window(event.window));
                    None
                }
                _ => return Err(check(UNSUPPORTED)),
            };
            if let Some(event) = event_native {
                commands.push(Command::Event(PalEvent::Input(event), context));
            }
        }
        if state.commands.len() + commands.len() > session::MAX_INPUT {
            return Err(check(CAPACITY));
        }
        state.commands.extend(commands);
        drop(state);
        owner.shared.wake.notify_all();
        Ok(())
    })
}
unsafe extern "C" fn control(handle: Session, command: u32, error: *mut Error) -> Status {
    session_boundary(handle, error, || {
        let owner = owner(handle)?;
        if owner.shared.cancelled() {
            return Err(check(CANCELLED));
        }
        let mut state = owner.shared.lock();
        if matches!(state.status, SESSION_FAILED | SESSION_FINISHED) {
            return Err(check(INVALID_STATE));
        }
        match command {
            CONTROL_PAUSE => state.paused = true,
            CONTROL_RESUME => state.paused = false,
            _ => return Err(check(UNSUPPORTED)),
        }
        drop(state);
        owner.shared.wake.notify_all();
        Ok(())
    })
}
unsafe extern "C" fn state(
    handle: Session,
    output: *mut SessionState,
    error: *mut Error,
) -> Status {
    session_boundary(handle, error, || {
        let owner = owner(handle)?;
        let state = owner.shared.lock();
        let epoch = owner.shared.context.load(Ordering::Acquire);
        *destination(output)? = SessionState {
            state: state.status,
            flags: 0,
            context_epoch: epoch,
            output_epoch: epoch,
            error: state.error,
        };
        Ok(())
    })
}
struct Lease {
    pixels: Arc<Vec<u8>>,
    count: Arc<AtomicUsize>,
}
unsafe extern "C" fn release(owner: *mut c_void) {
    if !owner.is_null() {
        let lease = Box::from_raw(owner.cast::<Lease>());
        lease.count.fetch_sub(1, Ordering::AcqRel);
        drop(lease);
    }
}
unsafe extern "C" fn frame(
    handle: Session,
    after: FrameIdentity,
    output: *mut FrameSurface,
    error: *mut Error,
) -> Status {
    session_boundary(handle, error, || {
        let owner = owner(handle)?;
        let output = destination(output)?;
        let state = owner.shared.lock();
        let identity = FrameIdentity {
            session_epoch: owner.shared.epoch,
            output_epoch: owner.shared.context.load(Ordering::Acquire),
            generation: 0,
        };
        *output = FrameSurface {
            state: FRAME_PENDING,
            reserved: 0,
            identity,
            timestamp_ns: 0,
            info: state.info,
            cpu: CpuLease {
                pixels: Bytes::EMPTY,
                owner: std::ptr::null_mut(),
                release: None,
            },
        };
        if after.session_epoch != 0 && after.session_epoch != owner.shared.epoch {
            return Err(check(STALE_CONTEXT));
        }
        let Some(frame) = &state.frame else {
            return Ok(());
        };
        output.identity = frame.identity;
        output.timestamp_ns = frame.timestamp;
        output.info = frame.info;
        if after == frame.identity {
            output.state = FRAME_UNCHANGED;
            return Ok(());
        }
        owner
            .leases
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 3).then_some(count + 1)
            })
            .map_err(|_| check(CAPACITY))?;
        let lease = Box::new(Lease {
            pixels: Arc::clone(&frame.pixels),
            count: Arc::clone(&owner.leases),
        });
        output.cpu = CpuLease {
            pixels: Bytes::new(&lease.pixels),
            owner: Box::into_raw(lease).cast(),
            release: Some(release),
        };
        output.state = FRAME_READY;
        Ok(())
    })
}
unsafe extern "C" fn cancel(handle: Session) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Ok(owner) = owner(handle) {
            owner.shared.cancel();
        }
    }));
}
unsafe extern "C" fn close(handle: Session, error: *mut Error) -> Status {
    boundary(error, || {
        reference::<SessionOwner>(handle.cast())?;
        drop(Box::from_raw(handle.cast::<SessionOwner>()));
        Ok(())
    })
}
unsafe extern "C" fn extension(_name: Bytes, _version: u32, output: *mut *const c_void) -> Status {
    if output.is_null() || !output.is_aligned() {
        return INVALID_ARGUMENT;
    }
    output.write(std::ptr::null());
    UNSUPPORTED
}

#[cfg(test)]
mod tests;
