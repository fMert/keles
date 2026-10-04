#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use http::Response;
use include_dir::{include_dir, Dir};
#[cfg(target_os = "windows")]
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
#[cfg(target_os = "windows")]
use wry::{PermissionKind, PermissionResponse, WebViewBuilder};

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
fn main() {
    linux::run();
}

static PKG: Dir = include_dir!("$CARGO_MANIFEST_DIR/../pkg");
static TEXTURES: Dir = include_dir!("$CARGO_MANIFEST_DIR/../assets/camel_images");

// Adapted from tauri-apps/wry, examples/custom_protocol.rs, lines 73-101.
// https://github.com/tauri-apps/wry/blob/792d0359ba6501a4fc360ece17de2ae42329a47c/examples/custom_protocol.rs#L73
// License: MIT (see ../LICENSE-MIT-Wry). Files are embedded instead of read from disk.
fn asset_response(path: &str) -> Response<Vec<u8>> {
    let content = match path {
        "/" | "/index.html" => Some(include_bytes!("../../index.html").as_slice()),
        "/assets/weapon_idle.png" => {
            Some(include_bytes!("../../assets/weapon_idle.png").as_slice())
        }
        "/assets/weapon_recoil.png" => {
            Some(include_bytes!("../../assets/weapon_recoil.png").as_slice())
        }
        "/assets/weapon_flash.png" => {
            Some(include_bytes!("../../assets/weapon_flash.png").as_slice())
        }
        "/assets/operative_turnaround.png" => {
            Some(include_bytes!("../../assets/operative_turnaround.png").as_slice())
        }
        "/assets/girl_turnaround.png" => {
            Some(include_bytes!("../../assets/girl_turnaround.png").as_slice())
        }
        "/assets/logo.png" => Some(include_bytes!("../../assets/logo.png").as_slice()),
        _ => path
            .strip_prefix("/pkg/")
            .and_then(|p| PKG.get_file(p))
            .or_else(|| {
                path.strip_prefix("/assets/camel_images/")
                    .and_then(|p| TEXTURES.get_file(p))
            })
            .map(|file| file.contents()),
    };
    let mimetype = if path.ends_with(".js") {
        "text/javascript"
    } else if path.ends_with(".wasm") {
        "application/wasm"
    } else if path.ends_with(".png") {
        "image/png"
    } else {
        "text/html; charset=utf-8"
    };
    Response::builder()
        .status(if content.is_some() { 200 } else { 404 })
        .header("Content-Type", mimetype)
        .body(content.unwrap_or(b"Not found").to_vec())
        .unwrap()
}

// Adapted from tauri-apps/wry, examples/custom_protocol.rs, lines 17-24, 34-67.
// https://github.com/tauri-apps/wry/blob/792d0359ba6501a4fc360ece17de2ae42329a47c/examples/custom_protocol.rs#L17
// License: MIT (see ../LICENSE-MIT-Wry). Uses the existing game and grants mouse capture only.
#[cfg(target_os = "windows")]
fn main() -> wry::Result<()> {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Keles")
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .build(&event_loop)
        .expect("Could not open the Keles window");
    let builder = WebViewBuilder::new()
        .with_custom_protocol("keles".into(), |_, request| {
            asset_response(request.uri().path()).map(Into::into)
        })
        .with_permission_handler(|permission| match permission {
            PermissionKind::PointerLock => PermissionResponse::Allow,
            _ => PermissionResponse::Deny,
        })
        .with_url("keles://localhost/");
    let _webview = builder.build(&window)?;
    event_loop.run(move |event, _, control_flow| {
        // Keep the window and WebView alive until the event loop exits.
        let _ = (&window, &_webview);
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_game_is_complete_and_paths_are_confined() {
        for (path, mime) in [
            ("/", "text/html; charset=utf-8"),
            ("/pkg/keles.js", "text/javascript"),
            ("/pkg/keles_bg.wasm", "application/wasm"),
            ("/assets/weapon_idle.png", "image/png"),
            ("/assets/weapon_recoil.png", "image/png"),
            ("/assets/weapon_flash.png", "image/png"),
            ("/assets/operative_turnaround.png", "image/png"),
            ("/assets/girl_turnaround.png", "image/png"),
            ("/assets/logo.png", "image/png"),
        ] {
            let response = asset_response(path);
            assert_eq!(response.status(), 200, "{path}");
            assert_eq!(response.headers()["Content-Type"], mime);
            assert!(!response.body().is_empty());
        }
        assert_eq!(&asset_response("/pkg/keles_bg.wasm").body()[..4], b"\0asm");
        for index in 0..34 {
            assert_eq!(
                asset_response(&format!("/assets/camel_images/{index}.png")).status(),
                200
            );
        }
        for path in [
            "/missing",
            "/pkg/../../Cargo.toml",
            "/assets/camel_images/../camel_original.glb",
        ] {
            assert_eq!(asset_response(path).status(), 404);
        }
    }
}
