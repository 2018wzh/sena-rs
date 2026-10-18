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

## License
This project is licensed under the MPL-2.0 License. See [LICENSE](./LICENSE-MPL-2.0) for details.
