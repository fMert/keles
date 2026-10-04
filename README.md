# Keles

The desktop application runs the same HTML/Rust/WASM client in its own window.
The HTML, WASM, generated loader and images are embedded in the executable;
no browser tab, local HTTP server or game server process is needed. Single
player works offline. For multiplayer, enter your remote server's `IP:port`
or `wss://` address in **Play → Multiplayer** as before.

Desktop downloads are on [GitHub Releases](https://github.com/fMert/keles/releases).
The Linux amd64 `.deb` targets Debian 13 or a compatible newer distribution:
install with `sudo apt install ./keles-desktop_0.2.1_amd64.deb`, then launch
**Keles** from the application menu or run `keles`. APT installs the required
system runtime dependencies. Linux bundles Chromium Embedded Framework (CEF);
Windows uses the system's Chromium-based WebView2. The Windows x64 executable is portable:
double-click `keles-0.2.1-windows-x64.exe`. Windows 10/11 requires the
[Microsoft Edge WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/),
which is usually already installed. The executable includes the MSVC runtime.
Keep the release's `LICENSE` and `THIRD-PARTY-LICENSES.txt` with the Windows
executable when redistributing it. The Debian package includes these notices.

Build the desktop client after rebuilding WASM from the project root:

```sh
wasm-pack build --target web --release
# Linux build prerequisites (Debian/Ubuntu):
sudo apt install build-essential cmake libx11-dev
cargo run --locked --manifest-path desktop/Cargo.toml -- --disable-setuid-sandbox
cargo test --locked --manifest-path desktop/Cargo.toml
cargo install cargo-deb --locked
cd desktop
cargo build --locked --release
strip --strip-unneeded target/release/libcef.so
cargo deb --locked --no-build --output ../dist/keles-desktop_0.2.1_amd64.deb
```

On Windows, install Rust with the MSVC toolchain and Visual Studio C++ build
tools, rebuild WASM as above, then build the portable executable:

```sh
cargo build --locked --release --manifest-path desktop/Cargo.toml --target x86_64-pc-windows-msvc --config 'target.x86_64-pc-windows-msvc.rustflags=["-C", "target-feature=+crt-static"]'
```

The result is `desktop/target/x86_64-pc-windows-msvc/release/keles-desktop.exe`.
From Linux, the same command can use `cargo xwin build` after installing
[cargo-xwin](https://github.com/rust-cross/cargo-xwin), Clang and LLD, and
running `rustup target add x86_64-pc-windows-msvc`.
WASM must be rebuilt **before** each desktop build so the embedded client is current.
The Linux build downloads the matching CEF runtime automatically. Its shared
library, resources, sandbox helper and licenses must accompany the executable;
`cargo deb` packages them. The Rust client remains embedded. No local HTTP
server is started: CEF serves `http://keles.localhost/` from embedded bytes.
This loopback origin supports pointer lock and existing `ws://` servers.
Mouse capture and local/loopback network permissions are allowed only for this
embedded game origin; other permission requests are denied. The development
command above uses Chromium's namespace sandbox without a root-owned helper;
the installed package uses its root-owned setuid sandbox helper.

Desktop source borrowing (all from Wry v0.57.0, commit
`792d0359ba6501a4fc360ece17de2ae42329a47c`, MIT):

| Repository | Exact source | Adaptation |
| --- | --- | --- |
| [tauri-apps/wry](https://github.com/tauri-apps/wry) | [`examples/custom_protocol.rs`, `get_wry_response`, lines 73–101](https://github.com/tauri-apps/wry/blob/792d0359ba6501a4fc360ece17de2ae42329a47c/examples/custom_protocol.rs#L73) | Asset path, MIME and response handling; embedded bytes replace filesystem reads, missing paths return 404. |
| [tauri-apps/wry](https://github.com/tauri-apps/wry) | [`examples/custom_protocol.rs`, `main`, lines 17–24 and 34–67](https://github.com/tauri-apps/wry/blob/792d0359ba6501a4fc360ece17de2ae42329a47c/examples/custom_protocol.rs#L17) | Windows native window, custom protocol and close event loop. |

Wry meets the project quality bar: it is Tauri's production webview library,
with over 1,000 commits, over 100 contributors, CI, maintained examples,
documentation, issue triage and security audits. Both adaptations include
source comments and `desktop/LICENSE-MIT-Wry`. Dependencies are used through
their public APIs: [Tao](https://github.com/tauri-apps/tao) (Apache-2.0) is
Tauri's maintained windowing library, with CI and over 100 contributors;
[include_dir](https://github.com/Michael-F-Bryan/include_dir) (MIT) has existed
since 2017, with over 200 commits, 14 contributors, tests and examples. No
implementation snippets were copied from these two dependencies.

Linux process setup and window creation are adapted from
[tauri-apps/cef-rs](https://github.com/tauri-apps/cef-rs), commit
`f044f89e561237033435743e5dbcbf486d0d10a9`, MIT:
[`examples/cefsimple/src/shared/mod.rs`, lines 26–27 and 36–74](https://github.com/tauri-apps/cef-rs/blob/f044f89e561237033435743e5dbcbf486d0d10a9/examples/cefsimple/src/shared/mod.rs#L36),
and [`shared/simple_app.rs`, lines 6–64, 66–73, 93–95, 156–166 and 182–188](https://github.com/tauri-apps/cef-rs/blob/f044f89e561237033435743e5dbcbf486d0d10a9/examples/cefsimple/src/shared/simple_app.rs#L6).
Only those setup snippets are adapted; asset streams and permissions use public
CEF APIs. The source includes a borrowing comment and `desktop/LICENSE-MIT-CEF-Rust`.
This is Tauri's maintained Rust CEF integration, with over 1,100 commits, 27
contributors, CI, runnable examples, a security policy and reviewed issues/PRs.
CEF itself is the established BSD-licensed Chromium embedding framework.
The window uses CEF Views with Chrome runtime style and no toolbar: Alloy style
rejected pointer lock with `UnknownError` on this machine. Only the English CEF
locale and the standard runtime files are packaged; debug symbols are removed.

The 0.2.0 Linux wrapper could run at 62 FPS on a 165 Hz NVIDIA/X11 display.
WebKit's GPU diagnostic reported a 60 Hz timer rather than hardware vblank,
and disabled accelerated compositing. Its fallback sleeps `1000 / 60` integer
milliseconds (16 ms), explaining the observed 62.5 FPS and uneven presentation
on a 165 Hz screen. Disabling its 60 FPS preference and requesting acceleration
did not fix this machine. Release 0.2.1 replaces the Linux WebKit engine with
CEF rather than changing the game's render loop or reporting artificial FPS.
The [WebKit timer implementation](https://github.com/WebKit/WebKit/blob/webkitgtk-2.52.6/Source/WebKit/UIProcess/glib/DisplayVBlankMonitorTimer.cpp)
documents that fallback.

Release 0.2.1 was checked on the actual NVIDIA/X11 165 Hz display: CEF ran
at 165 FPS, and the owner confirmed mouse capture and camera movement worked.
Pause/resume, nickname entry, local WebSocket connection and team selection
were also checked with the CEF client.
Raw input (`unadjustedMovement`) was rejected in this Linux environment;
leave it disabled to use normal mouse capture. The embedded asset and scoped
permission tests and desktop Clippy checks passed. Windows was rebuilt with
the same embedded client; Windows runtime behavior remains untested here.

Release 0.2.0 validation: the Linux application was run on a separate virtual
display with Mesa software rendering. Both maps, WASD, mouse capture/look,
shooting, pause, nickname entry, WebSocket connection to the existing local
relay and team selection were checked. The embedded asset test and desktop
Clippy checks passed. The Windows x64 MSVC build was cross-compiled and its
PE/GUI format, system DLL imports and embedded HTML/WASM were checked; it has
not been run on a Windows machine. The desktop package contains only the client.

Build the browser game from the project root:

```sh
wasm-pack build --target web --release
python3 -m http.server 8000
```

Open `http://localhost:8000/`. Choose **Play → Single player** and select
**Camel** or **Test**. Test is the original box map. Camel uses the supplied
Dust 2 model and its textures. Movement follows the model's floors and walls.
Multiplayer always loads Camel; install the rebuilt server binary too, since
it uses the same mesh for collision and shot obstruction.

Settings are available from the main menu and pause menu. Sensitivity uses
Minecraft Java's 0–200% scale and cubic mouse curve: 50% gives 0.05145° per
raw mouse count. Raw input requests unaccelerated pointer movement through the
browser's Pointer Lock API. If the browser rejects that request, disable Raw
input to use regular mouse input. HUD scale changes the text and health bars,
not the weapon image. Show FPS toggles the top-right counter. Show blood effects
controls the small hit and larger death bursts.

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
player's team along with positions, headings, and health. Each browser loads
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
persistent game data. If the game page is served over HTTPS, put the relay
behind a TLS reverse proxy and enter a `wss://` address.

Character sprites: the boys and girls four-view turnarounds
(`assets/operative_turnaround.png`, `assets/girl_turnaround.png`) and the
weapon images are supplied by the project owner.

Camel asset credit: **"de_dust2 - CS map" by vrchris**
([Sketchfab source](https://sketchfab.com/3d-models/de-dust2-cs-map-056008d59eb849a29c0ab6884c0c3d87)),
offered there under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
`assets/camel_original.glb` is the supplied original. `tools/convert` extracts
its mesh and images into `assets/camel_mesh.bin` and `assets/camel_images/` and
recenters its coordinates. The original textures are displayed without
replacement. The Sketchfab listing is the uploader's license claim; anyone
distributing the game should verify rights to the Counter-Strike-derived asset.
