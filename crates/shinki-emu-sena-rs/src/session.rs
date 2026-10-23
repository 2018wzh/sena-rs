use crate::config::{self, Settings};
use shinki_emu_abi::*;
use pal_vm::{Engine, EngineConfig, PalEvent, RuntimeStatus};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc, Condvar, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const STEP: Duration = Duration::from_millis(16);
pub const MAX_INPUT: usize = 4096;
static ACTIVE: AtomicBool = AtomicBool::new(false);
#[derive(Clone, Copy)]
pub struct Host(pub HostServices);
// The ABI requires thread-safe callbacks and storage alive through close/join.
unsafe impl Send for Host {}
unsafe impl Sync for Host {}
impl Host {
    pub fn cancelled(self, epoch: u64) -> bool {
        unsafe {
            (self
                .0
                .cancellation
                .is_cancelled
                .expect("validated cancellation"))(self.0.cancellation.context, epoch)
                != 0
        }
    }
    pub fn invalidate(self, epoch: u64, context: u64) {
        if let Some(invalidate) = self.0.window.invalidate {
            unsafe { invalidate(self.0.window.context, epoch, context) };
        }
    }
}
#[derive(Clone)]
pub struct Frame {
    pub identity: FrameIdentity,
    pub timestamp: u64,
    pub info: SurfaceInfo,
    pub pixels: Arc<Vec<u8>>,
}
pub enum Command {
    Event(PalEvent, u64),
    Window(WindowState),
}
pub struct State {
    pub status: u32,
    pub error: Error,
    pub paused: bool,
    pub suspended: bool,
    pub commands: VecDeque<Command>,
    pub frame: Option<Frame>,
    pub info: SurfaceInfo,
    pub started: bool,
}
pub struct Shared {
    pub state: Mutex<State>,
    pub wake: Condvar,
    pub stop: AtomicBool,
    pub context: AtomicU64,
    pub epoch: u64,
    pub host: Host,
}
impl Shared {
    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
    pub fn cancelled(&self) -> bool {
        self.stop.load(Ordering::Acquire) || self.host.cancelled(self.epoch)
    }
    pub fn cancel(&self) {
        self.stop.store(true, Ordering::Release);
        self.wake.notify_all();
        self.host
            .invalidate(self.epoch, self.context.load(Ordering::Acquire));
    }
    pub fn fail(&self, code: &str, message: &str) {
        let mut state = self.lock();
        state.status = SESSION_FAILED;
        state.error.set(code, message);
        state.commands.clear();
    }
}
struct Active;
impl Drop for Active {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}
pub struct SessionOwner {
    pub shared: Arc<Shared>,
    pub leases: Arc<AtomicUsize>,
    worker: Option<JoinHandle<()>>,
    _active: Active,
}
impl Drop for SessionOwner {
    fn drop(&mut self) {
        self.shared.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl SessionOwner {
    pub fn open(settings: Settings, request: &OpenRequest) -> Result<Self, (Status, String)> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| (INVALID_STATE, "a Sena session is already active".to_owned()))?;
        let active = Active;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                status: SESSION_RUNNING,
                error: Error::default(),
                paused: false,
                suspended: request.window.suspended != 0,
                commands: VecDeque::new(),
                frame: None,
                info: config::surface(&settings, 1280, 720),
                started: false,
            }),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            context: AtomicU64::new(1),
            epoch: request.session_epoch,
            host: Host(request.host),
        });
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("sena-native".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&thread_shared, settings, sender)
                }));
                match outcome {
                    Ok(Ok(())) => {
                        let mut state = thread_shared.lock();
                        if state.status != SESSION_FAILED {
                            state.status = SESSION_FINISHED;
                        }
                    }
                    Ok(Err(_)) if thread_shared.cancelled() => {
                        let mut state = thread_shared.lock();
                        if state.status != SESSION_FAILED {
                            state.status = SESSION_FINISHED;
                        }
                    }
                    Ok(Err(error)) => thread_shared.fail("sena.native", &format!("{error:#}")),
                    Err(_) => thread_shared.fail(
                        "sena.panic",
                        "native worker panicked; close to release the session",
                    ),
                }
            })
            .map_err(|error| (CORE_ERROR, error.to_string()))?;
        let mut owner = Self {
            shared,
            worker: Some(worker),
            leases: Arc::new(AtomicUsize::new(0)),
            _active: active,
        };
        loop {
            if owner.shared.cancelled() {
                return Err((CANCELLED, "session open cancelled".into()));
            }
            match receiver.recv_timeout(STEP) {
                Ok(()) => {
                    if owner.shared.cancelled() {
                        return Err((CANCELLED, "session open cancelled".into()));
                    }
                    return Ok(owner);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if let Some(worker) = owner.worker.take() {
                        let _ = worker.join();
                    }
                    let state = owner.shared.lock();
                    let message = String::from_utf8_lossy(
                        &state.error.message[..state.error.message_len as usize],
                    )
                    .into_owned();
                    return Err((CORE_ERROR, message));
                }
            }
        }
    }
    pub fn start(&self) {
        self.shared.lock().started = true;
        self.shared.wake.notify_all();
    }
}
fn checkpoint(shared: &Shared) -> anyhow::Result<()> {
    anyhow::ensure!(!shared.cancelled(), "session cancelled");
    Ok(())
}
fn run(shared: &Shared, settings: Settings, ready: mpsc::SyncSender<()>) -> anyhow::Result<()> {
    checkpoint(shared)?;
    let mut engine = Engine::new(EngineConfig {
        game_root: Some(settings.root.clone()),
        lang: settings.language,
        ..EngineConfig::default()
    })?;
    checkpoint(shared)?;
    anyhow::ensure!(
        engine.audio().is_enabled(),
        "native Kira audio device is unavailable"
    );
    let mut gpu = pollster::block_on(pal_vm::renderer::WgpuBackend::new_offscreen(
        winit::dpi::PhysicalSize::new(settings.width, settings.height),
    ))?;
    checkpoint(shared)?;
    engine.handle_event(PalEvent::Resized {
        width: settings.width,
        height: settings.height,
    });
    let (width, height) = engine.logical_size_from_config(1280, 720);
    let info = config::surface(&settings, width, height);
    info.validate()
        .map_err(|_| anyhow::anyhow!("invalid native logical/raster dimensions"))?;
    shared.lock().info = info;
    let mut window_request_id = 0u64;
    crate::platform::service(shared, &mut engine, &mut window_request_id)?;
    checkpoint(shared)?;
    // Three immutable CPU images bound retained output independently of the VM.
    let mut buffers: [Arc<Vec<u8>>; 3] = std::array::from_fn(|_| {
        Arc::new(Vec::with_capacity(
            settings.width as usize * settings.height as usize * 4,
        ))
    });
    ready.send(())?;
    let mut next_frame = Instant::now();
    let clock = Instant::now();
    let mut audio_paused = false;
    let mut load_generation = 0;
    let mut generation = 0u64;
    let mut commands = VecDeque::new();
    while !shared.cancelled() {
        let mut state = shared.lock();
        if !state.started {
            let _ = shared
                .wake
                .wait_timeout(state, STEP)
                .unwrap_or_else(|error| error.into_inner());
            continue;
        }
        std::mem::swap(&mut commands, &mut state.commands);
        drop(state);
        for command in commands.drain(..) {
            match command {
                Command::Window(window) => {
                    shared.lock().suspended = window.suspended != 0;
                    if window.focused == 0 || window.suspended != 0 {
                        engine.clear_input();
                    }
                    if let Some(runtime) = engine.runtime_mut() {
                        let system = runtime.system_state_mut();
                        let (x, y) = system.window_pos();
                        let mode = system.window_mode();
                        if window.drawable_width > 0 && window.drawable_height > 0 {
                            system.confirm_window_geometry(
                                mode,
                                x,
                                y,
                                window.drawable_width.min(i32::MAX as u32) as i32,
                                window.drawable_height.min(i32::MAX as u32) as i32,
                            );
                        }
                    }
                    // Manager output O never changes Engine's input/raster coordinate space R.
                }
                Command::Event(event, epoch) if epoch == shared.context.load(Ordering::Acquire) => {
                    engine.handle_event(event)
                }
                Command::Event(_, _) => {}
            }
        }
        let mut state = shared.lock();
        let paused = state.paused || state.suspended;
        if paused != audio_paused {
            if paused {
                engine.audio_mut().suspend();
                engine.clear_input();
            } else {
                engine.audio_mut().unsuspend();
                engine.clear_input();
            }
            audio_paused = paused;
            next_frame = Instant::now();
        }
        if paused {
            state.status = SESSION_PAUSED;
            let _ = shared
                .wake
                .wait_timeout(state, STEP)
                .unwrap_or_else(|error| error.into_inner());
            continue;
        }
        let now = Instant::now();
        if now < next_frame {
            let _ = shared
                .wake
                .wait_timeout(state, next_frame - now)
                .unwrap_or_else(|error| error.into_inner());
            continue;
        }
        drop(state);
        next_frame = now + STEP;
        checkpoint(shared)?;
        // Same fixed 16ms PAL tick as SenaApplication, with no catch-up after pause.
        let frame = engine.update_with_delta(STEP)?;
        engine.input_begin_frame();
        let new_load = engine
            .runtime()
            .map_or(0, |runtime| runtime.load_generation());
        let info = config::surface(
            &settings,
            frame.scene.logical_width,
            frame.scene.logical_height,
        );
        info.validate()
            .map_err(|_| anyhow::anyhow!("native scene dimensions are out of range"))?;
        let previous_context = {
            let mut state = shared.lock();
            if new_load != load_generation || state.info.logical != info.logical {
                load_generation = new_load;
                state.frame = None;
                state.commands.clear();
                state.info = info;
                generation = 0;
                Some(shared.context.fetch_add(1, Ordering::AcqRel))
            } else {
                None
            }
        };
        if let Some(previous) = previous_context {
            shared.host.invalidate(shared.epoch, previous);
            engine.clear_input();
        }
        crate::platform::service(shared, &mut engine, &mut window_request_id)?;
        checkpoint(shared)?;
        if let Some(buffer) = buffers
            .iter_mut()
            .find(|buffer| Arc::strong_count(buffer) == 1)
        {
            gpu.render(&frame.scene, pal_vm::RendererConfig::default().clear_color)?;
            gpu.read_rgba(Arc::get_mut(buffer).expect("worker owns unused image"))?;
            generation = generation
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("frame generation exhausted"))?;
            let mut state = shared.lock();
            state.info = info;
            state.frame = Some(Frame {
                identity: FrameIdentity {
                    session_epoch: shared.epoch,
                    output_epoch: shared.context.load(Ordering::Acquire),
                    generation,
                },
                timestamp: clock.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                info,
                pixels: Arc::clone(buffer),
            });
        }
        match &frame.runtime_status {
            RuntimeStatus::Faulted { .. }
            | RuntimeStatus::UnsupportedCommand { .. }
            | RuntimeStatus::UnsupportedExtCall { .. } => {
                anyhow::bail!("{}", frame.runtime_status);
            }
            RuntimeStatus::Halted { .. } => {
                shared.lock().status = SESSION_FINISHED;
                break;
            }
            RuntimeStatus::WaitClick { .. } | RuntimeStatus::WaitFrame { .. } => {
                shared.lock().status = SESSION_WAITING
            }
            _ => shared.lock().status = SESSION_RUNNING,
        }
    }
    engine.audio_mut().shutdown()?;
    Ok(())
}
