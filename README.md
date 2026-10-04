# Keles

Keles is a small multiplayer FPS written in Rust. It runs as a native desktop
program: [eframe](https://github.com/emilk/egui/tree/main/crates/eframe)
opens the window through winit and glutin, and the game draws with OpenGL
through [glow](https://github.com/grovesNL/glow). There is no browser engine,
web view or JavaScript. The images, mesh and map are embedded in the
executable; single player works offline.

Downloads are on [GitHub Releases](https://github.com/fMert/keles/releases).
The Linux amd64 `.deb` targets Debian 13 or a compatible newer distribution:
install with `sudo apt install ./keles-desktop_0.3.0_amd64.deb`, then launch
**Keles** from the application menu or run `keles`. APT installs the system
OpenGL, X11/Wayland and DejaVu font packages it needs. The Windows x64
executable is portable: double-click `keles-0.3.0-windows-x64.exe`. It needs
only Windows' own OpenGL driver and Segoe UI font; the MSVC runtime is
included. Keep the release's `LICENSE` and `THIRD-PARTY-LICENSES.txt` with
the Windows executable when redistributing it. The Debian package includes
these notices.

Build and run from the project root:

```sh
# Linux build prerequisites (Debian/Ubuntu):
sudo apt install build-essential
cargo run --release
cargo install cargo-deb --locked
cargo deb --locked --output dist/keles-desktop_0.3.0_amd64.deb
```

The Windows x64 executable can be cross-compiled from Linux with
[cargo-xwin](https://github.com/rust-cross/cargo-xwin) after installing Clang
and LLD and running `rustup target add x86_64-pc-windows-msvc`:

```sh
cargo xwin build --release --target x86_64-pc-windows-msvc --config 'target.x86_64-pc-windows-msvc.rustflags=["-C", "target-feature=+crt-static"]'
```

The result is `target/x86_64-pc-windows-msvc/release/keles.exe`.
`THIRD-PARTY-LICENSES.txt` is generated with
[cargo-about](https://github.com/EmbarkStudios/cargo-about) 0.9.2 from the
dependency tree. Fonts are read from the system (DejaVu Sans on Linux, Segoe
UI on Windows), so no font files are distributed.

Choose **Play → Single player** and select **Camel** or **Test**. Test is the
original box map. Camel uses the supplied Dust 2 model and its textures.
Movement follows the model's floors and walls. Multiplayer always loads Camel;
install the rebuilt server binary too, since it uses the same mesh for
collision and shot obstruction.

Settings are available from the main menu and pause menu. Sensitivity uses
Minecraft Java's 0–200% scale and cubic mouse curve and defaults to 50%
(0.05145° per raw mouse count). Mouse look always reads raw, unaccelerated
motion; while playing, the cursor is hidden and kept in the window. HUD scale
changes the text and health bars, not the weapon image. Show FPS toggles the
top-right counter. Show blood effects controls the small hit and larger death
bursts.

**Fullscreen** switches to borderless fullscreen. **Resolution** lists common
modes up to the monitor's size. In a window, choosing one resizes the window.
In fullscreen, the 3D scene renders at the chosen resolution and is stretched
over the screen, like a game's stretched resolution; menus and HUD stay sharp.
**Native** uses the window's own size. **FPS limit** caps the frame rate
(30–360) or removes the cap with **Unlimited**. The first value is the
monitor's refresh rate. The driver does not wait for vsync, so the limit is
the only frame pacing. Settings reset when the game restarts.

Source borrowing:

| Repository | Exact source | Adaptation |
| --- | --- | --- |
| [emilk/egui](https://github.com/emilk/egui) | [`crates/egui_demo_app/src/apps/custom3d_glow.rs`, lines 67–75](https://github.com/emilk/egui/blob/2ec7a836879b958f05ef542fab5a12a86250ef62/crates/egui_demo_app/src/apps/custom3d_glow.rs#L67) | egui paint callback; draws the off-screen scene over the window. |
| [emilk/egui](https://github.com/emilk/egui) | [`examples/hello_world/src/main.rs`, lines 8–21](https://github.com/emilk/egui/blob/2ec7a836879b958f05ef542fab5a12a86250ef62/examples/hello_world/src/main.rs#L8) | Native window options and `run_native` call. |
| [wasm-bindgen/wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen) | [`examples/webgl/src/lib.rs`, lines 89–134](https://github.com/wasm-bindgen/wasm-bindgen/blob/main/examples/webgl/src/lib.rs) | Shader compile and link checks, now through glow. |
| [WebGLSamples/WebGL2Samples](https://github.com/WebGLSamples/WebGL2Samples) | `transform_feedback_interleaved.html` lines 20–33, 52–84; `texture_format.html` lines 20–55 | GLSL shaders; desktop OpenGL gets a `#version 330 core` header. |

egui (commit `2ec7a836879b958f05ef542fab5a12a86250ef62`, tag 0.36.2) is
MIT OR Apache-2.0 (see `LICENSE-MIT-egui`). It meets the project quality bar:
over 30,000 stars, over 600 contributors, CI, maintained examples, issue
triage, and production users such as Rerun. The other two keep their earlier
notices (`LICENSE-MIT-wasm-bindgen`, `LICENSE-MIT-WebGL2Samples`).
Dependencies are used through their public APIs: eframe/egui/glow (MIT OR
Apache-2.0) for the window, OpenGL and menus; [ewebsock](https://github.com/rerun-io/ewebsock)
(MIT OR Apache-2.0, maintained by the egui/Rerun team, used by Rerun) for
WebSocket and `wss://` through tungstenite and rustls; [image](https://github.com/image-rs/image)
(MIT OR Apache-2.0, almost 400 contributors) for PNG decoding; and glam for
math.

Release 0.3.0 was checked on a separate Xvfb display with Mesa software
rendering and the Openbox window manager: menus, both maps, WASD, raw mouse
look, shooting, pause, fullscreen, windowed and fullscreen resolution changes
and the 60 FPS, 144 FPS and unlimited limits worked. The Windows executable
was cross-compiled and imports only system DLLs; it was not run on Windows.

Run the small multiplayer relay locally or on a VPS:

```sh
cargo build --release --manifest-path server/Cargo.toml
./server/target/release/keles-server 0.0.0.0:9001
```

For the VPS, copy only `server/target/release/keles-server` after building
for that machine and run `./keles-server 0.0.0.0:9001`. The Cargo build
directory is not needed at runtime.

Choose **Play → Multiplayer**, enter `IP_ADDRESS:9001` and a nickname, and
connect, then pick a team on the **Choose your team** screen. Boys and girls
use different four-view sprites (boys: `assets/operative_turnaround.png`,
girls: the supplied `assets/girl_turnaround.png`); the server relays each
player's team along with positions, headings, and health. Each client loads
the same map locally. Other players use four transparent 2D views selected
from their facing direction. Click to fire: a shot through a player's 3D
hitbox removes one of three health points, and boxes block shots. Your health
bar is at the bottom left.

Girls always spawn on the CT side and boys on the T side of the de_dust2
replica, at a random point on that team's terrace so players do not stack. The
terrace rectangles and floors live in `src/map.rs`
(`CAMEL_T_SPAWN` / `CAMEL_CT_SPAWN`); they are derived from the map's
`info_player_*` entities through the model's known root transform. A scoreboard
at the top counts each team's kills (girls pink, boys blue, your team marked).
Three hits kill a player; that player gets a **Respawn** button and returns to
their team's terrace, one kill goes to the shooter's team, and the first team
to 40 wins. The win or loss screen counts down 10 seconds, then the round
resets with scores cleared and everyone respawned. The server
accepts up to 16 simultaneous connections and keeps no
persistent game data. To use TLS, put the relay behind a TLS reverse proxy and enter a `wss://`
address.

Character sprites: the boys and girls four-view turnarounds
(`assets/operative_turnaround.png`, `assets/girl_turnaround.png`) and the
weapon images and the logo (`assets/logo.png`) are supplied by the
project owner.

Camel asset credit: **"de_dust2 - CS map" by vrchris**
([Sketchfab source](https://sketchfab.com/3d-models/de-dust2-cs-map-056008d59eb849a29c0ab6884c0c3d87)),
offered there under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
`assets/camel_original.glb` is the supplied original. `tools/convert` extracts
its mesh and images into `assets/camel_mesh.bin` and `assets/camel_images/` and
recenters its coordinates. The original textures are displayed without
replacement. The Sketchfab listing is the uploader's license claim; anyone
distributing the game should verify rights to the Counter-Strike-derived asset.
