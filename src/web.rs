use super::{vec2, Game, Screen, Vec2};
use wasm_bindgen::{prelude::*, JsCast};

pub fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

pub fn storage_get(key: &str) -> Option<String> {
    web_sys::window()?
        .local_storage()
        .ok()??
        .get_item(key)
        .ok()?
}

pub fn storage_set(key: &str, value: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(key, value);
    }
}

pub fn storage_remove(key: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.remove_item(key);
    }
}

pub fn canvas() -> web_sys::HtmlCanvasElement {
    document()
        .get_element_by_id("keles")
        .unwrap()
        .unchecked_into()
}

pub fn show_error(message: &str) {
    let element = document().get_element_by_id("loading").unwrap();
    element.set_text_content(Some(message));
    let retry = document().create_element("a").unwrap();
    retry.set_attribute("href", "").unwrap();
    retry.set_text_content(Some(" Yeniden yükle"));
    element.append_child(&retry).unwrap();
    element.set_attribute("style", "display:grid").ok();
}

pub fn server_url() -> Result<String, String> {
    let location = web_sys::window().unwrap().location();
    let path = document()
        .query_selector("meta[name=keles-websocket-path]")
        .ok()
        .flatten()
        .and_then(|element| element.get_attribute("content"))
        .ok_or("Bağlantı ayarı bulunamadı.")?;
    super::websocket_url(
        &location.protocol().unwrap_or_default(),
        &location.host().unwrap_or_default(),
        &path,
        cfg!(debug_assertions),
    )
    .map_err(str::to_owned)
}

fn pause(game: &mut Game) {
    game.keys = [false; 4];
    game.jump = false;
    game.mouse_delta = Vec2::ZERO;
    if game.screen == Screen::Playing {
        game.show_screen(Screen::Paused);
    }
}

#[wasm_bindgen(start)]
pub async fn start() -> Result<(), JsValue> {
    let runner = eframe::WebRunner::new();
    // Keep runtime failures readable even after the loading overlay is removed.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        show_error("Oyun durdu. Sayfayı yeniden yükleyin.");
        hook(info);
    }));
    let result = start_game(&runner).await;
    if result.is_err() {
        show_error("Oyun başlatılamadı. WebGL2 destekli bir masaüstü tarayıcısı kullanın.");
    }
    result
}

async fn start_game(runner: &eframe::WebRunner) -> Result<(), JsValue> {
    let canvas = canvas();
    runner
        .start(
            canvas.clone(),
            eframe::WebOptions {
                webgl_context_option: eframe::WebGlContextOption::WebGl2,
                dithering: false,
                ..Default::default()
            },
            Box::new(|cc| Ok(Box::new(Game::new(cc)))),
        )
        .await?;

    runner.add_event_listener(
        &document(),
        "mousemove",
        |event: web_sys::MouseEvent, app| {
            let game = app.app_mut::<Game>();
            if game.grabbed && game.screen == Screen::Playing {
                game.mouse_delta += vec2(event.movement_x() as f32, event.movement_y() as f32);
            }
        },
    )?;
    runner.add_event_listener(
        &document(),
        "pointerlockchange",
        |_: web_sys::Event, app| {
            let game = app.app_mut::<Game>();
            game.grabbed = document().pointer_lock_element().is_some();
            game.mouse_delta = Vec2::ZERO;
            if !game.grabbed {
                pause(game);
            }
        },
    )?;
    runner.add_event_listener(&document(), "pointerlockerror", |_: web_sys::Event, app| {
        let game = app.app_mut::<Game>();
        pause(game);
        game.status = "Fare kilitlenemedi. Devam et düğmesine tıklayın.".to_owned();
    })?;
    runner.add_event_listener(
        &web_sys::window().unwrap(),
        "blur",
        |_: web_sys::Event, app| {
            pause(app.app_mut::<Game>());
            document().exit_pointer_lock();
        },
    )?;
    runner.add_event_listener(&document(), "visibilitychange", |_: web_sys::Event, app| {
        if document().hidden() {
            pause(app.app_mut::<Game>());
            document().exit_pointer_lock();
        }
    })?;
    runner.add_event_listener(&document(), "fullscreenerror", |_: web_sys::Event, app| {
        app.app_mut::<Game>().status = "Tam ekran açılamadı. Yeniden deneyin.".to_owned();
    })?;
    runner.add_event_listener(&canvas, "webglcontextlost", |event: web_sys::Event, app| {
        event.prevent_default();
        let game = app.app_mut::<Game>();
        pause(game);
        game.disconnect();
        show_error("Görüntü bağlantısı kesildi. Sayfayı yeniden yükleyin.");
    })?;
    document()
        .get_element_by_id("loading")
        .unwrap()
        .set_attribute("style", "display:none")?;
    Ok(())
}
