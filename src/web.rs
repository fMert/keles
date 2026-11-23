use super::{vec2, Game, Screen, Vec2};
use std::cell::RefCell;
use wasm_bindgen::{prelude::*, JsCast};

thread_local! {
    static AUDIO: RefCell<Option<(web_sys::AudioContext, web_sys::AudioBuffer)>> = const { RefCell::new(None) };
}

// Web Audio API node setup pattern adapted from wasm-bindgen/examples/webaudio/src/lib.rs,
// lines 44-95: https://github.com/wasm-bindgen/wasm-bindgen/blob/main/examples/webaudio/src/lib.rs
// License: MIT (see LICENSE-MIT-wasm-bindgen). Synthesizes tactical FPS audio without asset files.
fn with_audio(volume: f32, f: impl FnOnce(&web_sys::AudioContext, &web_sys::AudioBuffer, f32)) {
    let vol = (volume / 100.0).clamp(0.0, 1.0);
    if vol <= 0.001 {
        return;
    }
    AUDIO.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let Ok(ctx) = web_sys::AudioContext::new() else {
                return;
            };
            let len = (ctx.sample_rate() * 0.45) as u32;
            let Ok(buf) = ctx.create_buffer(1, len, ctx.sample_rate()) else {
                return;
            };
            let mut seed: u32 = 0x9e37_79b9;
            let mut samples = Vec::with_capacity(len as usize);
            for _ in 0..len {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                samples.push((seed as f32 / u32::MAX as f32) * 2.0 - 1.0);
            }
            let _ = buf.copy_to_channel(&mut samples, 0);
            *slot = Some((ctx, buf));
        }
        if let Some((ctx, buf)) = slot.as_ref() {
            let _ = ctx.resume();
            f(ctx, buf, vol);
        }
    });
}

fn tone(
    ctx: &web_sys::AudioContext,
    kind: web_sys::OscillatorType,
    f0: f32,
    f1: f32,
    gain0: f32,
    start: f64,
    dur: f64,
) {
    let (Ok(osc), Ok(gain)) = (ctx.create_oscillator(), ctx.create_gain()) else {
        return;
    };
    osc.set_type(kind);
    let _ = osc.frequency().set_value_at_time(f0, start);
    let _ = osc
        .frequency()
        .exponential_ramp_to_value_at_time(f1.max(20.0), start + dur);
    let _ = gain.gain().set_value_at_time(gain0.max(0.0001), start);
    let _ = gain
        .gain()
        .exponential_ramp_to_value_at_time(0.0001, start + dur);
    let _ = osc.connect_with_audio_node(&gain);
    let _ = gain.connect_with_audio_node(&ctx.destination());
    let _ = osc.start_with_when(start);
    let _ = osc.stop_with_when(start + dur + 0.01);
}

fn noise(
    ctx: &web_sys::AudioContext,
    buf: &web_sys::AudioBuffer,
    filter_type: web_sys::BiquadFilterType,
    freq: f32,
    q: f32,
    gain0: f32,
    start: f64,
    dur: f64,
) {
    let (Ok(src), Ok(filter), Ok(gain)) = (
        ctx.create_buffer_source(),
        ctx.create_biquad_filter(),
        ctx.create_gain(),
    ) else {
        return;
    };
    src.set_buffer(Some(buf));
    filter.set_type(filter_type);
    let _ = filter.frequency().set_value_at_time(freq, start);
    let _ = filter.q().set_value_at_time(q, start);
    let _ = gain.gain().set_value_at_time(gain0.max(0.0001), start);
    let _ = gain
        .gain()
        .exponential_ramp_to_value_at_time(0.0001, start + dur);
    let _ = src.connect_with_audio_node(&filter);
    let _ = filter.connect_with_audio_node(&gain);
    let _ = gain.connect_with_audio_node(&ctx.destination());
    let _ = src.start_with_when_and_grain_offset_and_grain_duration(start, 0.0, dur + 0.01);
}

pub fn sfx_fire(volume: f32, distance: f32) {
    let atten = (1.0 / (1.0 + distance * 0.09)).clamp(0.08, 1.0);
    with_audio(volume, |ctx, buf, vol| {
        let t = ctx.current_time();
        let v = vol * atten;
        // AK-47 crack + low punch + metallic bolt snap.
        noise(
            ctx,
            buf,
            web_sys::BiquadFilterType::Bandpass,
            1150.0,
            0.7,
            0.42 * v,
            t,
            0.14,
        );
        noise(
            ctx,
            buf,
            web_sys::BiquadFilterType::Lowpass,
            420.0,
            0.9,
            0.36 * v,
            t,
            0.22,
        );
        tone(
            ctx,
            web_sys::OscillatorType::Sawtooth,
            175.0,
            42.0,
            0.32 * v,
            t,
            0.11,
        );
        tone(
            ctx,
            web_sys::OscillatorType::Triangle,
            2400.0,
            820.0,
            0.10 * v,
            t,
            0.03,
        );
    });
}

pub fn sfx_hit(volume: f32, kill: bool, streak: u32) {
    with_audio(volume, |ctx, _, vol| {
        let t = ctx.current_time();
        // Crisp tactical hitmarker tick.
        tone(
            ctx,
            web_sys::OscillatorType::Triangle,
            2100.0,
            1250.0,
            0.22 * vol,
            t,
            0.045,
        );
        if kill {
            // Escalating kill confirmation chime on multi-kill/streak.
            let step = (streak.saturating_sub(1).min(5)) as f32;
            let base = 520.0 * 1.12_f32.powf(step);
            tone(
                ctx,
                web_sys::OscillatorType::Sine,
                base,
                base * 1.5,
                0.22 * vol,
                t + 0.02,
                0.18,
            );
            tone(
                ctx,
                web_sys::OscillatorType::Triangle,
                base * 2.0,
                base * 2.0,
                0.12 * vol,
                t + 0.04,
                0.22,
            );
        }
    });
}

pub fn sfx_hurt(volume: f32) {
    with_audio(volume, |ctx, buf, vol| {
        let t = ctx.current_time();
        tone(
            ctx,
            web_sys::OscillatorType::Sine,
            130.0,
            48.0,
            0.34 * vol,
            t,
            0.14,
        );
        noise(
            ctx,
            buf,
            web_sys::BiquadFilterType::Lowpass,
            360.0,
            1.0,
            0.18 * vol,
            t,
            0.09,
        );
    });
}

pub fn sfx_step(volume: f32, land: bool) {
    with_audio(volume, |ctx, buf, vol| {
        let t = ctx.current_time();
        let g = if land { 0.14 } else { 0.065 } * vol;
        noise(
            ctx,
            buf,
            web_sys::BiquadFilterType::Bandpass,
            if land { 320.0 } else { 680.0 },
            1.2,
            g,
            t,
            0.055,
        );
    });
}

pub fn sfx_tick(volume: f32, go: bool) {
    with_audio(volume, |ctx, _, vol| {
        let t = ctx.current_time();
        if go {
            tone(
                ctx,
                web_sys::OscillatorType::Triangle,
                880.0,
                880.0,
                0.22 * vol,
                t,
                0.24,
            );
            tone(
                ctx,
                web_sys::OscillatorType::Sine,
                1320.0,
                1320.0,
                0.16 * vol,
                t + 0.04,
                0.26,
            );
        } else {
            tone(
                ctx,
                web_sys::OscillatorType::Sine,
                660.0,
                620.0,
                0.16 * vol,
                t,
                0.09,
            );
        }
    });
}

pub fn sfx_end(volume: f32, win: bool) {
    with_audio(volume, |ctx, _, vol| {
        let t = ctx.current_time();
        let notes = if win {
            [523.25, 659.25, 783.99, 1046.5]
        } else {
            [392.0, 349.23, 311.13, 261.63]
        };
        for (i, f) in notes.into_iter().enumerate() {
            tone(
                ctx,
                web_sys::OscillatorType::Triangle,
                f,
                f,
                0.16 * vol,
                t + i as f64 * 0.09,
                0.28,
            );
        }
    });
}

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
