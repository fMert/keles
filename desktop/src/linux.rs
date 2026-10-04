use cef::wrapper::{
    byte_read_handler::{ByteReadHandler, ByteStream},
    stream_resource_handler::StreamResourceHandler,
};
use cef::*;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

// Adapted from tauri-apps/cef-rs, examples/cefsimple/src/shared/simple_app.rs,
// lines 6-64, 66-73, 93-95, 156-166, 182-188, commit f044f89e561237033435743e5dbcbf486d0d10a9.
// https://github.com/tauri-apps/cef-rs/blob/f044f89e561237033435743e5dbcbf486d0d10a9/examples/cefsimple/src/shared/simple_app.rs#L6
// MIT (see ../LICENSE-MIT-CEF-Rust). Chrome style without a toolbar supports pointer lock.
wrap_window_delegate! {
    struct GameWindow { browser_view: RefCell<Option<BrowserView>>, }
    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 1280, height: 800 }
        }
    }
    impl PanelDelegate {}
    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let browser_view = self.browser_view.borrow();
            if let (Some(window), Some(browser_view)) = (window, browser_view.as_ref()) {
                window.set_title(Some(&"Keles".into()));
                window.add_child_view(Some(&mut View::from(browser_view)));
                window.show();
                View::from(browser_view).request_focus();
            }
        }
        fn can_close(&self, _window: Option<&mut Window>) -> i32 {
            self.browser_view.borrow().as_ref().and_then(|view| view.browser())
                .and_then(|browser| browser.host()).map_or(1, |host| host.try_close_browser())
        }
        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            self.browser_view.borrow_mut().take();
        }
        fn window_runtime_style(&self) -> RuntimeStyle { RuntimeStyle::CHROME }
    }
}

wrap_browser_view_delegate! {
    struct GameView {}
    impl ViewDelegate {}
    impl BrowserViewDelegate {
        fn browser_runtime_style(&self) -> RuntimeStyle { RuntimeStyle::CHROME }
        fn chrome_toolbar_type(&self, _view: Option<&mut BrowserView>) -> ChromeToolbarType { ChromeToolbarType::NONE }
    }
}

fn allows_game_permission(origin: &str, permissions: u32) -> bool {
    let Ok(origin) = origin.parse::<http::Uri>() else {
        return false;
    };
    let allowed = PermissionRequestTypes::POINTER_LOCK.get_raw()
        | PermissionRequestTypes::LOCAL_NETWORK.get_raw()
        | PermissionRequestTypes::LOOPBACK_NETWORK.get_raw();
    origin.scheme_str() == Some("http")
        && origin
            .authority()
            .is_some_and(|authority| authority.as_str() == "keles.localhost")
        && permissions != 0
        && permissions & !allowed == 0
}

wrap_scheme_handler_factory! {
    struct GameAssets;
    impl SchemeHandlerFactory {
        fn create(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>,
            _scheme: Option<&CefString>, request: Option<&mut Request>) -> Option<ResourceHandler> {
            let uri: http::Uri = CefString::from(&request?.url()).to_string().parse().ok()?;
            let response = super::asset_response(uri.path());
            let status = response.status().as_u16() as i32;
            let mime = response.headers()["Content-Type"].to_str().ok()?.split(';').next()?.to_owned();
            let mut reader = ByteReadHandler::new(Arc::new(Mutex::new(ByteStream::new(response.into_body()))));
            let stream = stream_reader_create_for_handler(Some(&mut reader));
            Some(StreamResourceHandler::new(status, "".into(), mime, None, stream))
        }
    }
}

wrap_life_span_handler! {
    struct GameLifetime;
    impl LifeSpanHandler {
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            quit_message_loop();
        }
    }
}

wrap_permission_handler! {
    struct GamePermissions;
    impl PermissionHandler {
        fn on_show_permission_prompt(&self, _browser: Option<&mut Browser>, _prompt_id: u64,
            origin: Option<&CefString>, permissions: u32,
            callback: Option<&mut PermissionPromptCallback>) -> i32 {
            if let Some(callback) = callback {
                let allowed = origin.is_some_and(|origin| allows_game_permission(&origin.to_string(), permissions));
                callback.cont(if allowed { PermissionRequestResult::ACCEPT } else { PermissionRequestResult::DENY });
            }
            1
        }
    }
}

wrap_client! {
    struct GameClient;
    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(GameLifetime::new()) }
        fn permission_handler(&self) -> Option<PermissionHandler> { Some(GamePermissions::new()) }
    }
}

// Adapted from tauri-apps/cef-rs, examples/cefsimple/src/shared/mod.rs, lines 26-27, 36-74,
// at f044f89e561237033435743e5dbcbf486d0d10a9.
// https://github.com/tauri-apps/cef-rs/blob/f044f89e561237033435743e5dbcbf486d0d10a9/examples/cefsimple/src/shared/mod.rs#L36
// MIT (see ../LICENSE-MIT-CEF-Rust). Only process setup and message loop are used.
pub fn run() {
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new();
    let exit_code = execute_process(Some(args.as_main_args()), None, std::ptr::null_mut());
    if exit_code >= 0 {
        std::process::exit(exit_code);
    }
    let executable = std::env::current_exe().expect("Cannot locate Keles");
    let adjacent = executable.parent().unwrap();
    let runtime = if adjacent.join("libcef.so").is_file() {
        adjacent.to_owned()
    } else {
        "/usr/lib/keles".into()
    };
    if runtime == std::path::Path::new("/usr/lib/keles") {
        // The Debian package installs Chromium's sandbox helper owned by root, mode 4755.
        std::env::set_var("CHROME_DEVEL_SANDBOX", runtime.join("chrome-sandbox"));
    }
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache")
        })
        .join("keles");
    let settings = Settings {
        locale: "en-US".into(),
        resources_dir_path: runtime.to_string_lossy().as_ref().into(),
        locales_dir_path: runtime.join("locales").to_string_lossy().as_ref().into(),
        root_cache_path: cache.to_string_lossy().as_ref().into(),
        ..Default::default()
    };
    assert_eq!(
        initialize(
            Some(args.as_main_args()),
            Some(&settings),
            None,
            std::ptr::null_mut()
        ),
        1
    );
    assert_eq!(
        register_scheme_handler_factory(
            Some(&"http".into()),
            Some(&"keles.localhost".into()),
            Some(&mut GameAssets::new())
        ),
        1
    );
    let browser_view = browser_view_create(
        Some(&mut GameClient::new()),
        Some(&"http://keles.localhost/".into()),
        Some(&BrowserSettings::default()),
        None,
        None,
        Some(&mut GameView::new()),
    );
    assert!(browser_view.is_some(), "Cannot create the Keles view");
    window_create_top_level(Some(&mut GameWindow::new(RefCell::new(browser_view))));
    run_message_loop();
    shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_are_confined_to_the_embedded_game() {
        let pointer = PermissionRequestTypes::POINTER_LOCK.get_raw();
        assert!(allows_game_permission("http://keles.localhost/", pointer));
        assert!(allows_game_permission("http://keles.localhost", pointer));
        assert!(allows_game_permission(
            "http://keles.localhost/",
            PermissionRequestTypes::LOOPBACK_NETWORK.get_raw()
        ));
        assert!(allows_game_permission(
            "http://keles.localhost/",
            PermissionRequestTypes::LOCAL_NETWORK.get_raw()
        ));
        assert!(!allows_game_permission("http://keles.localhost/", 0));
        assert!(!allows_game_permission(
            "http://keles.localhost.evil/",
            pointer
        ));
        assert!(!allows_game_permission("http://remote.example/", pointer));
        assert!(!allows_game_permission(
            "http://keles.localhost/",
            pointer | 1
        ));
    }
}
