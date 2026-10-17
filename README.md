# sena-rs

<img src="./assets/icon.png" alt="icon" align="left" width="100" style="margin-right: 10px;" />

Rust cross-platform implementation of the SoftPAL engine.
Specifically, targeted for the game "Koikake".

<br clear="left"/>

## Example screenshots

* sena-rs on macOS
![On Mac](./assets/screenshot-mac.png)

* sena-rs on iOS
![On iOS](./assets/screenshot-ios.png)

| Platform | Targets |
|---|---|
| Linux | x86_64, aarch64 |
| Windows | x86_64, ARM64 |
| macOS | DMG app bundle |
| iOS | arm64 device, arm64 simulator, x86_64 simulator |
| Android | arm64-v8a, x86_64 |
| WebAssembly | wasm32-unknown-unknown |

## Run

```bash
cargo run -p pal-vm --release --bin sena -- /path/to/game --nls sjis
```

## Library presentation and lifetime

`Engine` owns PAL execution, movies, native Kira audio, game menus and portable saves.
`renderer::WgpuBackend::new_offscreen` uses the same GPU compositor as the windowed
application; `read_rgba` copies its final image into reusable caller-owned storage.
It creates no window or event loop. Input coordinates must use that raster's size.

`AudioSystem::suspend` / `unsuspend` preserve script-paused channels. A caller that
pauses engine scheduling must also suspend audio and discard paused wall time.
On native targets `AudioSystem::shutdown` joins Kira's CPAL stream-management
worker; its owner-thread destructor provides the same cleanup on early errors.
Kira remains version 0.12.1, pinned to the narrowly patched native-owner fork.
Sena uses synchronously decoded `StaticSoundData`, not Kira streaming decoders.
`ScriptRuntime::load_generation` identifies successful native save restores.
Successful portable and original-save loads re-arm saving from the loaded
checkpoint, including loads made from a fresh title session. Subsequent saves
do not require an earlier in-session savepoint or retain an outgoing scene.
Platform owners drain `system_state_mut().take_window_requests()` and confirm actual
geometry with `confirm_window_geometry`; this never changes the renderer size.

## Astra Embedded

The `astra` branch adds `crates/astra-emu-sena`, a thin C ABI adapter for **Koikake**,
not a general SoftPAL compatibility claim. Its base is upstream
`37ab17558e0c7d26ebfb1dccfa981ba529a6630b`; generic offscreen/lifetime changes are on
the fork's `master`. The original MPL-2.0 source and history remain intact.

The descriptor identifies the engine as `family_id = "softpal"` and this emulator
as `plugin_id = "sena-native-koikake"`. These are independent identities: another
SoftPAL emulator may coexist under its own plugin ID. This does not expand Sena's
supported-title range or change its native save identity.

Build from this standalone fork or AstraEngine's native checkout. The shared ABI
is pinned to Git revision `15a39a626b1316cb6137a75e17e14a2ff2d1ba88` (C ABI 2); no host directory
layout is required. Kira 0.12.1 is pinned to
`523790dbb7ddea5ca432f375a9f3180923f2ea1e` with explicit native CPAL worker shutdown:

```bash
cargo build --release -p astra-emu-sena --features diagnostic-bridge
cargo test -p astra-emu-sena --lib
```

Dynamic loading exports `astra_emu_query` and installs the shared diagnostics bridge.
Static linking uses `astra_emu_sena::QUERY` without `diagnostic-bridge`, reusing the
host logger. The Manager owns the sole window/event loop and Present. A dedicated
native worker retains Sena's 16ms PAL cadence, original VM, game UI, Kira audio,
movie decoders and native save menus. It renders the complete native wgpu scene
offscreen and exports at most three independently owned immutable RGBA8 leases.
Leases remain releasable after close; close cancels platform services and joins
the worker before returning, permitting a new session in the same process.

`raster_width` / `raster_height` are startup-only (320–3840, defaults 1280×720).
`text_language` selects the native `jp` / `cn` / `tc` archive/text field.
The descriptor reports actual logical L and raster R; Manager resize affects O
without changing R. Pointer input is in displayed R pixels and goes through
Sena's existing coordinate conversion once. Save restoration invalidates queued
input and frame identity. Pause stops PAL time and suspends playing Kira channels.
No text/resource Hook or host audio mixer is used.

Prerequisites are a real native GPU and Kira audio device, a window-capable host,
and complete Koikake resources/fonts. The probe recognizes `koikake.exe`, or
`Game.exe` with `steam_appid.txt` containing Koikake's Steam app ID `2242710`.
`Game.exe` alone does not identify a supported SoftPAL title. Explicit opening
also supports an imported resource directory without these markers.
Native cursor-null is forwarded; upstream cursor-autohide remains only stored
state because upstream has no timer or documented units. Native aspect-mode sprite
transforms are retained, not translated into a host resize constraint.

These commands are verification entry points, not a completed playthrough claim.
Acceptance requires real final-frame comparison with the `sena` command above,
game-menu save/load, pause/audio, resize/pointer mapping, close/reopen with a held
CPU lease, and at least one real ending.

## License
This project is licensed under the MPL-2.0 License. See [LICENSE](./LICENSE-MPL-2.0) for details.
