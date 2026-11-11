#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::{collections::HashMap, sync::Arc};

use eframe::{
    egui::{
        self, pos2, vec2, Align2, Color32, FontId, Id, Key, LayerId, Order, Rect, RichText, Stroke,
        StrokeKind, Vec2,
    },
    egui_glow,
    glow::{self, HasContext},
};
use ewebsock::{WsEvent, WsMessage};
use glam::{camera, Mat4, Vec3};
// Shared with the server; each side uses only part of it.
#[allow(dead_code)]
mod map;
use map::Map;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(not(target_arch = "wasm32"))]
mod web {
    pub fn storage_get(_: &str) -> Option<String> {
        None
    }
    pub fn storage_set(_: &str, _: &str) {}
    pub fn storage_remove(_: &str) {}
}

fn valid_username(name: &str) -> bool {
    (3..=16).contains(&name.len())
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn valid_password(password: &str) -> bool {
    (8..=64).contains(&password.chars().count()) && !password.chars().any(char::is_control)
}

fn auth_error(code: &str) -> &'static str {
    match code {
        "bad_username" => "Kullanıcı adı 3–16 harf, rakam, _ veya - içermeli.",
        "bad_password" => "Şifre 8–64 karakter olmalı; kontrol karakteri içermemeli.",
        "taken" => "Bu kullanıcı adı zaten alınmış. Başka bir ad seçin.",
        "wrong" => "Kullanıcı adı veya şifre hatalı.",
        "online" => "Bu hesap başka bir pencerede açık. Önce oradan çıkış yapın.",
        "rate_limited" => "Çok fazla deneme yaptınız. Biraz bekleyip yeniden deneyin.",
        "expired" => "Oturumunuz sona erdi. Lütfen yeniden giriş yapın.",
        "full" => "Sunucu şu anda dolu veya işlem tamamlanamadı. Daha sonra deneyin.",
        _ => "Giriş yapılamadı. Yeniden deneyin.",
    }
}

fn websocket_url(
    protocol: &str,
    host: &str,
    path: &str,
    development: bool,
) -> Result<String, &'static str> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['?', '#'])
        || path.chars().any(|c| c.is_whitespace() || c == '\\')
    {
        return Err("WebSocket yolu geçersiz.");
    }
    let scheme = match protocol {
        "https:" => "wss",
        "http:" => "ws",
        _ => return Err("Oyun HTTP veya HTTPS üzerinden açılmalı."),
    };
    let hostname = host.rsplit_once(':').map_or(host, |(name, _)| name);
    if development && protocol == "http:" && matches!(hostname, "localhost" | "127.0.0.1" | "[::1]")
    {
        return Ok(format!("ws://{hostname}:9001{path}"));
    }
    Ok(format!("{scheme}://{host}{path}"))
}

#[cfg(test)]
mod tests {
    use super::websocket_url;

    #[test]
    fn websocket_address_keeps_production_on_the_page_origin() {
        assert_eq!(
            websocket_url("https:", "example.org", "/keles", false).unwrap(),
            "wss://example.org/keles"
        );
        assert_eq!(
            websocket_url("http:", "localhost:8080", "/socket", false).unwrap(),
            "ws://localhost:8080/socket"
        );
        assert_eq!(
            websocket_url("http:", "localhost:8080", "/keles", true).unwrap(),
            "ws://localhost:9001/keles"
        );
        assert_eq!(
            websocket_url("https:", "example.org:8443", "/socket", true).unwrap(),
            "wss://example.org:8443/socket"
        );
        for path in [
            "//other.org",
            "ws://other.org",
            "/bad path",
            "/x#fragment",
            "/\\other",
        ] {
            assert!(websocket_url("https:", "example.org", path, false).is_err());
        }
    }

    #[test]
    fn authentication_validation_accepts_pipes_and_rejects_controls() {
        assert!(super::valid_username("User_123-abc"));
        for name in ["ab", "bad name", "Türkçe", "abcdefghijklmnopq"] {
            assert!(!super::valid_username(name));
        }
        assert!(super::valid_password("a|b|c|d|"));
        assert!(super::valid_password(&"ş".repeat(64)));
        assert!(!super::valid_password("short"));
        assert!(!super::valid_password("password\n"));
        assert!(!super::valid_password(&"a".repeat(65)));
        assert_eq!(
            super::auth_error("wrong"),
            "Kullanıcı adı veya şifre hatalı."
        );
    }
}

// GLSL lines from WebGLSamples/WebGL2Samples/samples/transform_feedback_interleaved.html,
// lines 20-33 and 52-84; the vertex shader combines its MVP and color snippets.
// https://github.com/WebGLSamples/WebGL2Samples/blob/master/samples/transform_feedback_interleaved.html
// License: MIT (see LICENSE-MIT-WebGL2Samples).
const VERTEX_SHADER: &str = r#"#version 300 es
#define POSITION_LOCATION 0
#define COLOR_POSITION 3
precision highp float;
precision highp int;
uniform mat4 MVP;
layout(location = POSITION_LOCATION) in vec4 position;
layout(location = COLOR_POSITION) in vec4 color;
layout(location = 4) in vec2 texcoord;
out vec4 v_color;
out vec2 v_uv;
void main()
{
    gl_Position = MVP * position;
    v_color = color;
    v_uv = texcoord;
}
"#;
const FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;
precision highp int;
in vec4 v_color;
in vec2 v_uv;
uniform sampler2D diffuse;
uniform bool use_texture;
out vec4 color;
void main()
{
    color = v_color;
    if (use_texture) color.rgb *= texture(diffuse, v_uv).rgb;
}
"#;

// GLSL from WebGLSamples/WebGL2Samples/samples/texture_format.html, lines 20-55.
// https://github.com/WebGLSamples/WebGL2Samples/blob/master/samples/texture_format.html
// License: MIT (see LICENSE-MIT-WebGL2Samples).
const SPRITE_VERTEX_SHADER: &str = r#"#version 300 es
#define POSITION_LOCATION 0
#define TEXCOORD_LOCATION 4
precision highp float;
precision highp int;
uniform mat4 MVP;
layout(location = POSITION_LOCATION) in vec2 position;
layout(location = TEXCOORD_LOCATION) in vec2 texcoord;
out vec2 v_st;
void main()
{
    v_st = texcoord;
    gl_Position = MVP * vec4(position, 0.0, 1.0);
}
"#;
const SPRITE_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;
precision highp int;
uniform sampler2D diffuse;
in vec2 v_st;
out vec4 color;
void main()
{
    color = texture(diffuse, v_st);
}
"#;

// Menu and HUD colors after the logo: charred black panels and a
// crimson-to-amber glow.
const CRIMSON: Color32 = Color32::from_rgb(0xff, 0x4a, 0x1c);
const TEXT: Color32 = Color32::from_rgb(0xff, 0xe6, 0xda);
const STATUS: Color32 = Color32::from_rgb(0xf0, 0xa9, 0x8a);
const TITLE: Color32 = Color32::from_rgb(0xff, 0x7a, 0x2c);

const VERTICES: [f32; 24] = [
    -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 1.0, 1.0, 1.0, -1.0, 1.0, 1.0, -1.0, -1.0, -1.0, 1.0, -1.0,
    -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, -1.0,
];
const SPRITE_VERTICES: [f32; 24] = [
    -1.0, -1.0, 0.0, 1.0, 1.0, -1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0, 1.0, 1.0,
    1.0, 0.0, -1.0, 1.0, 0.0, 0.0,
];
const GRAVITY: f32 = 20.0;
const JUMP_SPEED: f32 = 6.0;
const FIRE_COOLDOWN_MS: f64 = 500.0;
const FALL_LIMIT: f32 = -20.0;

// Exact transparent cutouts in the supplied 1536x1024 four-view image:
// front, back, left-facing, right-facing. Coordinates are [left, top, right, bottom).
const OPERATIVE_VIEWS: [[f32; 4]; 4] = [
    [30.0, 51.0, 375.0, 937.0],
    [412.0, 54.0, 745.0, 946.0],
    [747.0, 55.0, 1135.0, 953.0],
    [1167.0, 54.0, 1526.0, 951.0],
];

// Girls team uses the user-supplied 1448x1086 four-view image in the same
// front/back/left/right order. Bounds are the four figure blobs detected by
// connected-component analysis, matching how the boys views were cut.
const GIRL_VIEWS: [[f32; 4]; 4] = [
    [26.0, 67.0, 384.0, 991.0],
    [392.0, 63.0, 744.0, 990.0],
    [694.0, 73.0, 1074.0, 1004.0],
    [1090.0, 76.0, 1431.0, 1008.0],
];

// Common monitor modes; ones larger than the monitor are hidden.
const RESOLUTIONS: [[u32; 2]; 13] = [
    [800, 600],
    [1024, 768],
    [1280, 720],
    [1280, 800],
    [1280, 1024],
    [1366, 768],
    [1440, 900],
    [1600, 900],
    [1680, 1050],
    [1920, 1080],
    [1920, 1200],
    [2560, 1440],
    [3840, 2160],
];
// 0 is unlimited.
const FPS_LIMITS: [u32; 9] = [30, 60, 75, 120, 144, 165, 240, 360, 0];

// Original CC BY 4.0 images embedded in the supplied Sketchfab GLB.
macro_rules! camel_images {
    ($($index:literal)*) => {
        [$(include_bytes!(concat!("../assets/camel_images/", $index, ".png")).as_slice()),*]
    };
}
const CAMEL_IMAGES: [&[u8]; 34] = camel_images!(
    0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33
);

// Minecraft Java's 0–200% slider maps to 0–1 before its cubic mouse curve.
// Formula documented from the game's mouse path at:
// https://modrinth.com/mod/tweak-mouse-sensitivity ("Original Code").
fn minecraft_radians_per_count(percent: f32) -> f32 {
    let value = percent / 200.0;
    let curve = value * 0.6 + 0.2;
    (curve * curve * curve * 1.2).to_radians()
}
const INDICES: [u16; 36] = [
    0, 1, 2, 0, 2, 3, 5, 4, 7, 5, 7, 6, 1, 5, 6, 1, 6, 2, 4, 0, 3, 4, 3, 7, 3, 2, 6, 3, 6, 7, 4, 5,
    1, 4, 1, 0,
];

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Boot,
    Auth,
    Lobby,
    Queue,
    MatchIntro,
    Playing,
    Paused,
    Settings,
    Dead,
    Result,
    RankedResult,
    Disconnected,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Deathmatch,
    Ranked,
}
#[derive(Clone, Copy, PartialEq)]
enum Field {
    Username,
    Password,
    Confirm,
}
struct Account {
    name: String,
    rating: u32,
    wins: u32,
    losses: u32,
}
struct RankedResult {
    result: String,
    reason: String,
    old: u32,
    rating: u32,
    delta: i64,
    at: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Team {
    Boys,
    Girls,
}

impl Team {
    fn color(self) -> Color32 {
        match self {
            Team::Boys => Color32::from_rgb(0x4a, 0x90, 0xff),
            Team::Girls => Color32::from_rgb(0xff, 0x66, 0xcc),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Team::Boys => "Erkekler",
            Team::Girls => "Kızlar",
        }
    }
}

struct RemotePlayer {
    name: String,
    x: f32,
    z: f32,
    y: f32,
    // Latest 32 Hz update; x, y and z ease toward it each frame.
    target: Vec3,
    yaw: f32,
    health: u8,
    team: Team,
    kills: u32,
    deaths: u32,
}

struct BloodEffect {
    position: Vec3,
    at: f64,
    death: bool,
}

struct CamelPart {
    first: i32,
    count: i32,
    image: usize,
}

// A player's heading and the viewer's position select the visible side.
// Each quarter-turn switches to one of the four supplied views.
fn operative_view(player: &RemotePlayer, viewer: Vec3) -> usize {
    let dx = viewer.x - player.x;
    let dz = viewer.z - player.z;
    let distance = (dx * dx + dz * dz).sqrt().max(0.0001);
    let forward_x = player.yaw.cos();
    let forward_z = player.yaw.sin();
    let facing = (forward_x * dx + forward_z * dz) / distance;
    if facing >= std::f32::consts::FRAC_1_SQRT_2 {
        0
    } else if facing <= -std::f32::consts::FRAC_1_SQRT_2 {
        1
    } else if forward_x * dz - forward_z * dx < 0.0 {
        2
    } else {
        3
    }
}

struct Game {
    gl: Arc<glow::Context>,
    program: glow::Program,
    vao: glow::VertexArray,
    camel_vao: glow::VertexArray,
    camel_parts: Vec<CamelPart>,
    camel_textures: Vec<glow::Texture>,
    transform: glow::UniformLocation,
    texture_enabled: glow::UniformLocation,
    sprite_program: glow::Program,
    sprite_vao: glow::VertexArray,
    operative_vaos: Vec<glow::VertexArray>,
    girl_vaos: Vec<glow::VertexArray>,
    sprite_transform: glow::UniformLocation,
    textures: [glow::Texture; 3],
    operative_texture: glow::Texture,
    girl_texture: glow::Texture,
    // The scene is drawn here at the chosen resolution, then stretched to the window.
    framebuffer: glow::Framebuffer,
    color: glow::Texture,
    depth: glow::Renderbuffer,
    target: [u32; 2],
    logo: egui::TextureHandle,
    shot_at: f64,
    position: Vec3,
    ground: Vec3,
    yaw: f32,
    pitch: f32,
    keys: [bool; 4],
    jump: bool,
    vertical: f32,
    last_frame: f64,
    screen: Screen,
    map: Map,
    username: String,
    password: String,
    confirm: String,
    register_tab: bool,
    remember: bool,
    show_password: bool,
    field_error: Option<(Field, String)>,
    touched: [bool; 3],
    focus_auth: bool,
    settings_back: Screen,
    mode: Mode,
    account: Option<Account>,
    token: String,
    auth_request: String,
    pending: Option<(&'static str, f64)>,
    online: u32,
    dm_players: u32,
    queued: u32,
    top: Vec<(String, u32, u32, u32)>,
    last_info: f64,
    queue_since: f64,
    opponent: Option<(String, u32)>,
    go_at: f64,
    ranked_ends_at: f64,
    ranked_started: bool,
    kill_target: u32,
    ranked_result: Option<RankedResult>,
    kill_feed: Vec<(String, String, Team, Team, f64)>,
    hitmark_at: f64,
    killed_by: Option<String>,
    died_at: f64,
    kills: u32,
    deaths: u32,
    toasts: Vec<(String, f64, bool)>,
    crosshair: (usize, f32, usize),
    confirm_leave: bool,
    status: String,
    sensitivity: f32,
    hud_scale: f32,
    show_fps: bool,
    show_blood: bool,
    fullscreen: bool,
    resolution: Option<[u32; 2]>,
    fps_limit: u32,
    fps: u32,
    fps_since: f64,
    fps_frames: u32,
    health: u8,
    self_id: Option<u32>,
    socket: Option<(ewebsock::WsSender, ewebsock::WsReceiver)>,
    connected: bool,
    team: Option<Team>,
    winner: Option<Team>,
    result_deadline: f64,
    score: [u32; 2],
    last_sent: f64,
    remote: HashMap<u32, RemotePlayer>,
    blood: Vec<BloodEffect>,
    local_hit_at: f64,
    grabbed: bool,
    mouse_delta: Vec2,
    connect_deadline: f64,
}

impl Game {
    fn new(cc: &eframe::CreationContext) -> Self {
        let gl = cc.gl.clone().expect("Keles needs OpenGL");
        let ctx = &cc.egui_ctx;
        ctx.set_theme(egui::Theme::Dark);
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(TEXT);
        visuals.extreme_bg_color = Color32::from_rgb(0x16, 0x08, 0x06);
        visuals.selection.bg_fill = CRIMSON;
        for (widget, fill) in [
            (
                &mut visuals.widgets.inactive,
                Color32::from_rgb(0x24, 0x0b, 0x07),
            ),
            (
                &mut visuals.widgets.hovered,
                Color32::from_rgb(0xa3, 0x26, 0x0c),
            ),
            (
                &mut visuals.widgets.active,
                Color32::from_rgb(0x3d, 0x0d, 0x05),
            ),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.bg_stroke = Stroke::new(1.0, CRIMSON);
            widget.corner_radius = 0.into();
        }
        ctx.set_visuals(visuals);
        // The previous desktop font, now embedded; Bitstream Vera license, DejaVu changes public domain.
        // https://github.com/dejavu-fonts/dejavu-fonts (binary asset; see assets/DejaVu-LICENSE.txt).
        let mut fonts = egui::FontDefinitions::empty();
        fonts.font_data.insert(
            "dejavu".to_owned(),
            Arc::new(egui::FontData::from_static(include_bytes!(
                "../assets/DejaVuSans.ttf"
            ))),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.insert(family, vec!["dejavu".to_owned()]);
        }
        ctx.set_fonts(fonts);
        let ([width, height], logo) = decode(include_bytes!("../assets/logo.png"));
        let logo = ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([width, height], &logo),
            egui::TextureOptions::LINEAR,
        );

        let camel_parts = include_str!("../assets/camel_parts.txt")
            .lines()
            .map(|line| {
                let mut fields = line.split_whitespace();
                CamelPart {
                    first: fields.next().unwrap().parse().unwrap(),
                    count: fields.next().unwrap().parse().unwrap(),
                    image: fields
                        .next()
                        .unwrap()
                        .split('.')
                        .next()
                        .unwrap()
                        .parse()
                        .unwrap(),
                }
            })
            .collect();

        let mut game = unsafe {
            let program = link_program(&gl, VERTEX_SHADER, FRAGMENT_SHADER);
            let sprite_program = link_program(&gl, SPRITE_VERTEX_SHADER, SPRITE_FRAGMENT_SHADER);
            let vao = vertex_array(&gl, &floats(&VERTICES), 0, &[(0, 3, 0)]);
            let indices = gl.create_buffer().expect("No index buffer");
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(indices));
            let index_bytes: Vec<u8> = INDICES.iter().flat_map(|i| i.to_ne_bytes()).collect();
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, &index_bytes, glow::STATIC_DRAW);
            // Mesh extracted from the user's Sketchfab GLB by tools/convert (CC BY 4.0).
            let camel_vao = vertex_array(
                &gl,
                include_bytes!("../assets/camel_mesh.bin"),
                20,
                &[(0, 3, 0), (4, 2, 12)],
            );
            let sprite_vao =
                vertex_array(&gl, &floats(&SPRITE_VERTICES), 16, &[(0, 2, 0), (4, 2, 8)]);
            let weapon = |png| texture(&gl, png, glow::CLAMP_TO_EDGE);
            Game {
                transform: gl.get_uniform_location(program, "MVP").unwrap(),
                texture_enabled: gl.get_uniform_location(program, "use_texture").unwrap(),
                sprite_transform: gl.get_uniform_location(sprite_program, "MVP").unwrap(),
                program,
                vao,
                camel_vao,
                camel_parts,
                camel_textures: CAMEL_IMAGES
                    .iter()
                    .map(|png| texture(&gl, png, glow::REPEAT))
                    .collect(),
                sprite_program,
                sprite_vao,
                operative_vaos: operative_vaos(&gl, &OPERATIVE_VIEWS, 1536.0, 1024.0),
                girl_vaos: operative_vaos(&gl, &GIRL_VIEWS, 1448.0, 1086.0),
                textures: [
                    weapon(include_bytes!("../assets/weapon_idle.png")),
                    weapon(include_bytes!("../assets/weapon_flash.png")),
                    weapon(include_bytes!("../assets/weapon_recoil.png")),
                ],
                operative_texture: weapon(include_bytes!("../assets/operative_turnaround.png")),
                girl_texture: weapon(include_bytes!("../assets/girl_turnaround.png")),
                framebuffer: gl.create_framebuffer().expect("No framebuffer"),
                color: gl.create_texture().expect("No scene texture"),
                depth: gl.create_renderbuffer().expect("No depth buffer"),
                target: [0, 0],
                gl,
                logo,
                shot_at: f64::NEG_INFINITY,
                position: Vec3::new(0.0, 1.7, 5.0),
                ground: Vec3::new(0.0, 1.7, 5.0),
                yaw: -std::f32::consts::FRAC_PI_2,
                pitch: 0.0,
                keys: [false; 4],
                jump: false,
                vertical: 0.0,
                last_frame: 0.0,
                screen: Screen::Auth,
                map: Map::Camel,
                username: web::storage_get("keles.username").unwrap_or_default(),
                password: String::new(),
                confirm: String::new(),
                register_tab: false,
                remember: true,
                show_password: false,
                field_error: None,
                touched: [false; 3],
                focus_auth: true,
                settings_back: Screen::Lobby,
                mode: Mode::Deathmatch,
                account: None,
                token: String::new(),
                auth_request: String::new(),
                pending: None,
                online: 0,
                dm_players: 0,
                queued: 0,
                top: Vec::new(),
                last_info: f64::NEG_INFINITY,
                queue_since: 0.0,
                opponent: None,
                go_at: 0.0,
                ranked_ends_at: 0.0,
                ranked_started: false,
                kill_target: map::WIN_SCORE,
                ranked_result: None,
                kill_feed: Vec::new(),
                hitmark_at: f64::NEG_INFINITY,
                killed_by: None,
                died_at: 0.0,
                kills: 0,
                deaths: 0,
                toasts: Vec::new(),
                crosshair: (0, 8.0, 0),
                confirm_leave: false,
                status: String::new(),
                sensitivity: 50.0,
                hud_scale: 200.0,
                show_fps: true,
                show_blood: true,
                fullscreen: false,
                resolution: None,
                fps_limit: 0,
                fps: 0,
                fps_since: 0.0,
                fps_frames: 0,
                health: 3,
                self_id: None,
                socket: None,
                connected: false,
                team: None,
                winner: None,
                result_deadline: 0.0,
                score: [0, 0],
                last_sent: 0.0,
                remote: HashMap::new(),
                blood: Vec::new(),
                local_hit_at: f64::NEG_INFINITY,
                grabbed: false,
                mouse_delta: Vec2::ZERO,
                connect_deadline: 0.0,
            }
        };
        game.load_settings();
        if let Some(token) = web::storage_get("keles.token") {
            game.token = token;
            game.screen = Screen::Boot;
        }
        game
    }

    fn disconnect(&mut self) {
        self.socket = None;
        self.remote.clear();
        self.blood.clear();
        self.local_hit_at = f64::NEG_INFINITY;
        self.connected = false;
        self.team = None;
        self.winner = None;
        self.self_id = None;
        self.score = [0, 0];
    }

    fn send(&mut self, text: String) {
        if let Some((sender, _)) = &mut self.socket {
            sender.send(WsMessage::Text(text));
        }
    }

    fn show_screen(&mut self, screen: Screen) {
        self.screen = screen;
        self.keys = [false; 4];
        self.jump = false;
        self.mouse_delta = Vec2::ZERO;
        #[cfg(target_arch = "wasm32")]
        if !matches!(screen, Screen::Playing | Screen::MatchIntro)
            && !(screen == Screen::Boot && self.grabbed)
        {
            web::document().exit_pointer_lock();
        }
        match screen {
            Screen::Playing | Screen::Lobby => self.status.clear(),
            Screen::Dead => self.status = "Yeniden doğmayı bekliyorsun.".to_owned(),
            _ => {}
        }
    }

    fn lock_pointer(&mut self) {
        #[cfg(target_arch = "wasm32")]
        web::canvas().request_pointer_lock();
    }

    fn request(&mut self, text: &'static str) {
        if self.pending.is_none() {
            self.send(text.to_owned());
            self.pending = Some((text, self.last_frame + 10_000.0));
        }
    }

    fn lobby(&mut self) {
        self.request("LOBBY");
    }

    fn logout(&mut self) {
        self.send("LOGOUT".to_owned());
        web::storage_remove("keles.token");
        self.token.clear();
        self.account = None;
        self.pending = None;
        self.disconnect();
        self.show_screen(Screen::Auth);
        self.focus_auth = true;
    }

    fn submit_auth(&mut self) {
        self.touched = [true; 3];
        self.field_error = self.validate_auth();
        if self.field_error.is_some() || self.pending.is_some() {
            return;
        }
        let auth = format!(
            "{}|{}|{}",
            if self.register_tab {
                "REGISTER"
            } else {
                "LOGIN"
            },
            self.username,
            self.password
        );
        if let Err(error) = self.connect(auth, false) {
            self.status = error;
        }
    }

    fn validate_auth(&self) -> Option<(Field, String)> {
        if self.touched[0] && !valid_username(&self.username) {
            return Some((
                Field::Username,
                "3–16 harf, rakam, _ veya - kullanın.".to_owned(),
            ));
        }
        if self.touched[1] && !valid_password(&self.password) {
            return Some((
                Field::Password,
                "Şifreniz 8–64 karakter olmalı; kontrol karakteri içermemeli.".to_owned(),
            ));
        }
        if self.register_tab && self.touched[2] && self.password != self.confirm {
            return Some((Field::Confirm, "Şifreler eşleşmiyor.".to_owned()));
        }
        None
    }

    fn escape(&mut self) {
        if self.pending.is_some() {
            return;
        }
        match self.screen {
            Screen::Playing => self.show_screen(Screen::Paused),
            Screen::Paused => {
                if self.confirm_leave {
                    self.confirm_leave = false;
                    return;
                }
                self.lock_pointer();
                self.show_screen(if self.winner.is_some() {
                    Screen::Result
                } else if self.health == 0 {
                    Screen::Dead
                } else {
                    Screen::Playing
                });
            }
            Screen::Queue => self.request("UNQUEUE"),
            Screen::Settings => self.show_screen(self.settings_back),
            Screen::MatchIntro | Screen::Dead | Screen::Result => self.show_screen(Screen::Paused),
            _ => {}
        }
    }

    fn input(&mut self, ctx: &egui::Context) {
        let (escape, jump, fire, keys, focused) = ctx.input(|i| {
            (
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::Space),
                i.pointer.button_pressed(egui::PointerButton::Primary),
                [Key::W, Key::A, Key::S, Key::D].map(|key| i.key_down(key)),
                i.focused,
            )
        });
        if escape {
            self.escape();
        }
        if self.screen != Screen::Playing || (self.mode == Mode::Ranked && !self.ranked_started) {
            return;
        }
        if !self.grabbed {
            self.keys = [false; 4];
            self.jump = false;
            return;
        }
        let delta = std::mem::take(&mut self.mouse_delta);
        self.keys = keys;
        self.jump = jump;
        let radians = minecraft_radians_per_count(self.sensitivity);
        self.yaw += delta.x * radians;
        self.pitch = (self.pitch - delta.y * radians).clamp(-1.5, 1.5);
        if !focused {
            self.show_screen(Screen::Paused);
            return;
        }
        if fire && self.health > 0 && self.last_frame - self.shot_at >= FIRE_COOLDOWN_MS {
            self.shot_at = self.last_frame;
            if self.connected {
                self.send(format!("FIRE|{:.5}|{:.5}", self.yaw, self.pitch));
            }
        }
    }

    fn connect(&mut self, auth: String, boot: bool) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        let url = web::server_url()?;
        #[cfg(not(target_arch = "wasm32"))]
        let url = "ws://127.0.0.1:9001/keles".to_owned();
        self.disconnect();
        self.account = None;
        self.auth_request = auth;
        self.socket = Some(
            ewebsock::connect(url, Default::default())
                .map_err(|_| "Bağlantı kurulamadı. Yeniden deneyin.")?,
        );
        self.connect_deadline = self.last_frame + 10_000.0;
        self.pending = Some(("AUTH", self.connect_deadline));
        if boot {
            self.show_screen(Screen::Boot);
        } else {
            self.show_screen(Screen::Auth);
        }
        self.status = "Giriş yapılıyor…".to_owned();
        Ok(())
    }

    fn poll_socket(&mut self) {
        while let Some(event) = self
            .socket
            .as_ref()
            .and_then(|(_, events)| events.try_recv())
        {
            match event {
                WsEvent::Opened => {
                    let auth = std::mem::take(&mut self.auth_request);
                    self.send(auth);
                }
                WsEvent::Message(WsMessage::Text(text)) => self.message(&text),
                WsEvent::Message(_) => {}
                WsEvent::Error(_) | WsEvent::Closed => {
                    let logged_in = self.account.is_some();
                    self.pending = None;
                    self.disconnect();
                    self.show_screen(if logged_in {
                        Screen::Disconnected
                    } else {
                        Screen::Auth
                    });
                    self.status = "Bağlantı kesildi. Yeniden bağlanabilirsiniz.".to_owned();
                }
            }
        }
    }

    fn clear_match(&mut self) {
        self.remote.clear();
        self.kill_feed.clear();
        self.blood.clear();
        self.kills = 0;
        self.deaths = 0;
        self.winner = None;
        self.score = [0, 0];
        self.team = None;
        self.self_id = None;
        self.connected = false;
        self.killed_by = None;
        self.shot_at = f64::NEG_INFINITY;
        self.local_hit_at = f64::NEG_INFINITY;
        self.yaw = -std::f32::consts::FRAC_PI_2;
        self.pitch = 0.0;
        if self.mode == Mode::Deathmatch {
            self.kill_target = map::WIN_SCORE;
        }
    }

    fn player_label(&self, id: u32) -> (String, Team) {
        if self.self_id == Some(id) {
            (self.username.clone(), self.team.unwrap_or(Team::Boys))
        } else {
            self.remote
                .get(&id)
                .map(|p| (p.name.clone(), p.team))
                .unwrap_or(("Oyuncu".to_owned(), Team::Boys))
        }
    }

    fn message(&mut self, text: &str) {
        let mut parts = text.split('|');
        match parts.next() {
            Some("AUTH_OK") => {
                let fields: Vec<_> = parts.collect();
                if fields.len() != 5 {
                    return;
                }
                let (Ok(rating), Ok(wins), Ok(losses)) =
                    (fields[2].parse(), fields[3].parse(), fields[4].parse())
                else {
                    return;
                };
                self.username = fields[0].to_owned();
                self.token = fields[1].to_owned();
                self.account = Some(Account {
                    name: self.username.clone(),
                    rating,
                    wins,
                    losses,
                });
                web::storage_set("keles.username", &self.username);
                if self.remember {
                    web::storage_set("keles.token", &self.token);
                } else {
                    web::storage_remove("keles.token");
                }
                self.password.clear();
                self.confirm.clear();
                self.pending = None;
                self.field_error = None;
                self.show_screen(Screen::Lobby);
                self.send("INFO".to_owned());
                self.last_info = self.last_frame;
            }
            Some("AUTH_ERR") => {
                let code = parts.next().unwrap_or_default();
                if matches!(code, "expired" | "online") {
                    web::storage_remove("keles.token");
                }
                self.pending = None;
                self.disconnect();
                self.account = None;
                self.show_screen(Screen::Auth);
                self.focus_auth = true;
                self.status = auth_error(code).to_owned();
            }
            Some("INFO") => {
                let values: Vec<_> = parts.filter_map(|v| v.parse::<u32>().ok()).collect();
                if values.len() != 6 {
                    return;
                }
                self.online = values[0];
                self.dm_players = values[1];
                self.queued = values[2];
                if let Some(a) = &mut self.account {
                    a.rating = values[3];
                    a.wins = values[4];
                    a.losses = values[5];
                }
                if self.pending.is_some_and(|(action, _)| action == "LOBBY") {
                    self.pending = None;
                    self.clear_match();
                    self.show_screen(Screen::Lobby);
                }
            }
            Some("TOP") => {
                self.top = parts
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .filter_map(|row| {
                        let fields: Vec<_> = row.split(':').collect();
                        if fields.len() != 4 {
                            return None;
                        }
                        Some((
                            fields[0].to_owned(),
                            fields[1].parse().ok()?,
                            fields[2].parse().ok()?,
                            fields[3].parse().ok()?,
                        ))
                    })
                    .collect();
            }
            Some("QUEUED") => {
                self.pending = None;
                self.queue_since = self.last_frame;
                self.show_screen(Screen::Queue);
            }
            Some("UNQUEUED") => {
                self.pending = None;
                self.show_screen(Screen::Lobby);
            }
            Some("MATCH") => {
                let fields: Vec<_> = parts.collect();
                if fields.len() != 6 {
                    return;
                }
                let (Ok(opp), Ok(me), Ok(target), Ok(seconds), Ok(countdown)) = (
                    fields[1].parse(),
                    fields[2].parse(),
                    fields[3].parse(),
                    fields[4].parse::<f64>(),
                    fields[5].parse::<f64>(),
                ) else {
                    return;
                };
                self.clear_match();
                self.pending = None;
                self.mode = Mode::Ranked;
                self.ranked_started = false;
                self.opponent = Some((fields[0].to_owned(), opp));
                if let Some(a) = &mut self.account {
                    a.rating = me;
                }
                self.kill_target = target;
                self.go_at = self.last_frame + countdown * 1000.0;
                self.ranked_ends_at = self.go_at + seconds * 1000.0;
                self.show_screen(Screen::MatchIntro);
            }
            Some("GO") => {
                self.ranked_started = true;
                if self.screen == Screen::MatchIntro && self.grabbed {
                    self.show_screen(Screen::Playing);
                }
            }
            Some("RANKED_END") => {
                let fields: Vec<_> = parts.collect();
                if fields.len() != 6 {
                    return;
                }
                let (Ok(rating), Ok(delta), Ok(wins), Ok(losses)) = (
                    fields[2].parse(),
                    fields[3].parse(),
                    fields[4].parse(),
                    fields[5].parse(),
                ) else {
                    return;
                };
                let old = self.account.as_ref().map_or(rating, |a| a.rating);
                if let Some(a) = &mut self.account {
                    a.rating = rating;
                    a.wins = wins;
                    a.losses = losses;
                }
                self.ranked_result = Some(RankedResult {
                    result: fields[0].to_owned(),
                    reason: fields[1].to_owned(),
                    old,
                    rating,
                    delta,
                    at: self.last_frame,
                });
                self.pending = None;
                self.connected = false;
                self.ranked_started = false;
                self.show_screen(Screen::RankedResult);
                self.send("INFO".to_owned());
            }
            Some("HITMARK") => {
                self.hitmark_at = self.last_frame;
            }
            Some("KILL") => {
                let (Some(Ok(killer)), Some(Ok(victim))) = (
                    parts.next().map(str::parse::<u32>),
                    parts.next().map(str::parse::<u32>),
                ) else {
                    return;
                };
                let (killer_name, killer_team) = self.player_label(killer);
                let (victim_name, victim_team) = self.player_label(victim);
                if self.self_id == Some(killer) {
                    self.kills += 1;
                } else if let Some(p) = self.remote.get_mut(&killer) {
                    p.kills += 1;
                }
                if self.self_id == Some(victim) {
                    self.deaths += 1;
                    self.killed_by = Some(killer_name.clone());
                } else if let Some(p) = self.remote.get_mut(&victim) {
                    p.deaths += 1;
                }
                if self.kill_feed.len() == 5 {
                    self.kill_feed.remove(0);
                }
                self.kill_feed.push((
                    killer_name,
                    victim_name,
                    killer_team,
                    victim_team,
                    self.last_frame,
                ));
            }
            Some("WELCOME") => {
                if let (Some(id), Some(x), Some(z), Some(y), Some(health), Some(team)) = (
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                ) {
                    if let (Ok(id), Ok(x), Ok(z), Ok(y), Ok(health)) = (
                        id.parse::<u32>(),
                        x.parse::<f32>(),
                        z.parse::<f32>(),
                        y.parse::<f32>(),
                        health.parse::<u8>(),
                    ) {
                        self.position = Vec3::new(x, y + map::EYE_HEIGHT, z);
                        self.ground = self.position;
                        self.vertical = 0.0;
                        self.self_id = Some(id);
                        self.health = health;
                        self.connected = true;
                        self.team = Some(if team == "girls" {
                            Team::Girls
                        } else {
                            Team::Boys
                        });
                        self.pending = None;
                        if self.mode == Mode::Ranked {
                            self.show_screen(Screen::MatchIntro);
                        } else {
                            self.show_screen(if self.grabbed {
                                Screen::Playing
                            } else {
                                Screen::Paused
                            });
                        }
                    }
                }
            }
            Some("SCORE") => {
                if let (Some(Ok(boys)), Some(Ok(girls))) = (
                    parts.next().map(str::parse::<u32>),
                    parts.next().map(str::parse::<u32>),
                ) {
                    if self.mode == Mode::Deathmatch && boys == 0 && girls == 0 {
                        self.kills = 0;
                        self.deaths = 0;
                        for p in self.remote.values_mut() {
                            p.kills = 0;
                            p.deaths = 0;
                        }
                    }
                    self.score = [boys, girls];
                }
            }
            Some("WIN") => {
                if let Some(winner) = parts.next() {
                    let remaining = parts
                        .next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(10.0);
                    self.winner = Some(if winner == "girls" {
                        Team::Girls
                    } else {
                        Team::Boys
                    });
                    self.result_deadline = self.last_frame + remaining * 1000.0;
                    self.show_screen(Screen::Result);
                }
            }
            Some("AT") => {
                if let (Some(x), Some(y), Some(z)) = (parts.next(), parts.next(), parts.next()) {
                    if let (Ok(x), Ok(y), Ok(z)) =
                        (x.parse::<f32>(), y.parse::<f32>(), z.parse::<f32>())
                    {
                        self.position = Vec3::new(x, y + map::EYE_HEIGHT, z);
                        self.ground = self.position;
                        self.vertical = 0.0;
                        if self.pending.is_some_and(|(action, _)| action == "RESPAWN") {
                            self.pending = None;
                        }
                        self.shot_at = f64::NEG_INFINITY;
                        // AT also starts a new round, so a past win must not block
                        // the death screen.
                        self.winner = None;
                        // Leave an open pause or settings menu alone.
                        if !matches!(self.screen, Screen::Paused | Screen::Settings) {
                            self.show_screen(if self.grabbed {
                                Screen::Playing
                            } else {
                                Screen::Paused
                            });
                        }
                    }
                }
            }
            Some("ADD") => {
                let (
                    Some(id),
                    Some(name),
                    Some(x),
                    Some(z),
                    Some(y),
                    Some(yaw),
                    Some(health),
                    Some(team),
                ) = (
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                )
                else {
                    return;
                };
                if let (Ok(id), Ok(x), Ok(z), Ok(y), Ok(yaw), Ok(health)) = (
                    id.parse::<u32>(),
                    x.parse::<f32>(),
                    z.parse::<f32>(),
                    y.parse::<f32>(),
                    yaw.parse::<f32>(),
                    health.parse::<u8>(),
                ) {
                    self.remote.insert(
                        id,
                        RemotePlayer {
                            name: name.to_owned(),
                            x,
                            z,
                            y,
                            target: Vec3::new(x, y, z),
                            yaw,
                            health,
                            kills: parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
                            deaths: parts.next().and_then(|v| v.parse().ok()).unwrap_or(0),
                            team: if team == "girls" {
                                Team::Girls
                            } else {
                                Team::Boys
                            },
                        },
                    );
                }
            }
            Some("MOVE") => {
                let (Some(id), Some(x), Some(z), Some(y), Some(yaw)) = (
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                    parts.next(),
                ) else {
                    return;
                };
                if let (Ok(id), Ok(x), Ok(z), Ok(y), Ok(yaw)) = (
                    id.parse::<u32>(),
                    x.parse::<f32>(),
                    z.parse::<f32>(),
                    y.parse::<f32>(),
                    yaw.parse::<f32>(),
                ) {
                    if let Some(player) = self.remote.get_mut(&id) {
                        player.target = Vec3::new(x, y, z);
                        // Respawns teleport; only walking is smoothed.
                        if player
                            .target
                            .distance(Vec3::new(player.x, player.y, player.z))
                            > 5.0
                        {
                            (player.x, player.y, player.z) = (x, y, z);
                        }
                        player.yaw = yaw;
                    }
                }
            }
            Some("LEAVE") => {
                if let Some(Ok(id)) = parts.next().map(str::parse::<u32>) {
                    self.remote.remove(&id);
                }
            }
            Some("HP") => {
                if let (Some(Ok(id)), Some(Ok(health))) = (
                    parts.next().map(str::parse::<u32>),
                    parts.next().map(str::parse::<u8>),
                ) {
                    if health > 3 {
                        return;
                    }
                    let mut hit_position = None;
                    if self.self_id == Some(id) {
                        if health < self.health {
                            hit_position = Some(Vec3::new(
                                self.position.x,
                                self.position.y - 0.6,
                                self.position.z,
                            ));
                            if self.show_blood {
                                self.local_hit_at = self.last_frame;
                            }
                        }
                        self.health = health;
                        if health == 0 && self.winner.is_none() {
                            self.died_at = self.last_frame;
                            self.show_screen(Screen::Dead);
                        }
                    } else if let Some(player) = self.remote.get_mut(&id) {
                        if health < player.health {
                            hit_position = Some(Vec3::new(player.x, player.y + 1.1, player.z));
                        }
                        player.health = health;
                    }
                    if let Some(position) = hit_position {
                        if self.show_blood {
                            if self.blood.len() == 8 {
                                self.blood.remove(0);
                            }
                            self.blood.push(BloodEffect {
                                position,
                                at: self.last_frame,
                                death: health == 0,
                            });
                        }
                    }
                }
            }
            Some("ERROR") => {
                self.pending = None;
                self.status = "Odaya girilemedi. Oda dolu olabilir; yeniden deneyin.".to_owned();
                self.toasts
                    .push((self.status.clone(), self.last_frame, true));
                self.show_screen(Screen::Lobby);
            }
            _ => {}
        }
    }

    // Moves the player and renders the scene into the off-screen target.
    // Returns false on menus that show no scene.
    fn draw(&mut self, now: f64, [width, height]: [u32; 2]) -> bool {
        let dt = ((now - self.last_frame) / 1000.0).clamp(0.0, 0.05) as f32;
        self.last_frame = now;
        let blend = (dt * 15.0).min(1.0);
        for player in self.remote.values_mut() {
            let eased = Vec3::new(player.x, player.y, player.z).lerp(player.target, blend);
            (player.x, player.y, player.z) = (eased.x, eased.y, eased.z);
        }
        if matches!(self.screen, Screen::Auth | Screen::Boot) {
            return false;
        }
        let forward = Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin());
        let right = Vec3::new(-forward.z, 0.0, forward.x);
        let movement = forward * (self.keys[0] as i32 - self.keys[2] as i32) as f32
            + right * (self.keys[3] as i32 - self.keys[1] as i32) as f32;
        if self.screen == Screen::Playing && self.health > 0 {
            let radius = 0.28;
            let feet = self.position.y - map::EYE_HEIGHT;
            if movement.length_squared() > 0.0 {
                let step = movement.normalize() * 4.0 * dt;
                // Landing or stepping down beside a wall can leave the body inside
                // it. Any move that does not get closer is then allowed, and steps
                // no longer than the gap cannot cross a wall.
                let mut gap =
                    map::wall_gap(self.map, self.position.x, self.position.z, feet, radius);
                for (dx, dz) in [(step.x, 0.0), (0.0, step.z)] {
                    let limit = if gap < radius {
                        gap.max(0.0)
                    } else {
                        f32::INFINITY
                    };
                    let x = self.position.x + dx.clamp(-limit, limit);
                    let z = self.position.z + dz.clamp(-limit, limit);
                    let next = map::wall_gap(self.map, x, z, feet, radius);
                    if next >= radius || next >= gap {
                        (self.position.x, self.position.z, gap) = (x, z, next);
                    }
                }
            }
            let feet = self.position.y - map::EYE_HEIGHT;
            if self.vertical == 0.0 {
                if let Some(floor) = map::floor_at(
                    self.map,
                    self.position.x,
                    self.position.z,
                    feet,
                    map::STEP_UP,
                ) {
                    if floor > feet {
                        self.position.y = floor + map::EYE_HEIGHT;
                    }
                }
            }
            if self.jump {
                if self.vertical == 0.0 {
                    self.vertical = JUMP_SPEED;
                }
                self.jump = false;
            }
            self.vertical -= GRAVITY * dt;
            let feet = self.position.y - map::EYE_HEIGHT;
            let landing = self.position.y + self.vertical * dt - map::EYE_HEIGHT;
            let floor = map::floor_at(
                self.map,
                self.position.x,
                self.position.z,
                feet,
                map::STEP_UP,
            );
            if let Some(floor) = floor.filter(|floor| landing <= *floor) {
                self.position.y = floor + map::EYE_HEIGHT;
                self.vertical = 0.0;
                self.ground = self.position;
            } else {
                self.position.y += self.vertical * dt;
            }
            // Back to the last floor, which the server also knows, not a fixed spawn.
            if self.position.y < FALL_LIMIT {
                self.position = self.ground;
                self.vertical = 0.0;
            }
        }
        if self.screen == Screen::Playing && self.connected && now - self.last_sent >= 31.25 {
            self.send(format!(
                "POS|{:.3}|{:.3}|{:.3}|{:.3}",
                self.position.x,
                self.position.z,
                self.position.y - map::EYE_HEIGHT,
                self.yaw
            ));
            self.last_sent = now;
        }

        let direction = Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        );
        let projection = camera::rh::proj::opengl::perspective(
            70.0_f32.to_radians(),
            width as f32 / height as f32,
            0.1,
            100.0,
        );
        let view = camera::rh::view::look_to_mat4(self.position, direction, Vec3::Y);
        let vp = projection * view;

        let gl = &self.gl;
        unsafe {
            if self.target != [width, height] {
                self.target = [width, height];
                gl.bind_texture(glow::TEXTURE_2D, Some(self.color));
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    width as i32,
                    height as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(self.depth));
                gl.renderbuffer_storage(
                    glow::RENDERBUFFER,
                    glow::DEPTH_COMPONENT24,
                    width as i32,
                    height as i32,
                );
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(self.color),
                    0,
                );
                gl.framebuffer_renderbuffer(
                    glow::FRAMEBUFFER,
                    glow::DEPTH_ATTACHMENT,
                    glow::RENDERBUFFER,
                    Some(self.depth),
                );
            }
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));
            gl.viewport(0, 0, width as i32, height as i32);
            gl.disable(glow::SCISSOR_TEST);
            gl.clear_color(0.58, 0.77, 0.93, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            gl.enable(glow::DEPTH_TEST);
            gl.use_program(Some(self.program));
            gl.active_texture(glow::TEXTURE0);
            gl.uniform_1_i32(Some(&self.texture_enabled), 0);
            gl.bind_vertex_array(Some(self.vao));
            for block in map::blocks(self.map) {
                let position = Vec3::from_array(block.center);
                let scale = Vec3::from_array(block.half);
                let model = Mat4::from_translation(position) * Mat4::from_scale(scale);
                gl.uniform_matrix_4_f32_slice(
                    Some(&self.transform),
                    false,
                    &(vp * model).to_cols_array(),
                );
                gl.vertex_attrib_4_f32(3, block.color[0], block.color[1], block.color[2], 1.0);
                gl.draw_elements(glow::TRIANGLES, 36, glow::UNSIGNED_SHORT, 0);
            }
            if self.map == Map::Camel {
                gl.bind_vertex_array(Some(self.camel_vao));
                let camel_vp = vp * Mat4::from_scale(Vec3::splat(map::CAMEL_SCALE));
                gl.uniform_matrix_4_f32_slice(
                    Some(&self.transform),
                    false,
                    &camel_vp.to_cols_array(),
                );
                gl.uniform_1_i32(Some(&self.texture_enabled), 1);
                gl.vertex_attrib_4_f32(3, 1.0, 1.0, 1.0, 1.0);
                for part in &self.camel_parts {
                    gl.bind_texture(glow::TEXTURE_2D, Some(self.camel_textures[part.image]));
                    gl.draw_arrays(glow::TRIANGLES, part.first, part.count);
                }
                gl.bind_vertex_array(Some(self.vao));
            }
            gl.uniform_1_i32(Some(&self.texture_enabled), 0);
            self.blood
                .retain(|effect| now - effect.at < if effect.death { 800.0 } else { 420.0 });
            if self.show_blood {
                for effect in &self.blood {
                    let age = ((now - effect.at) / 1000.0).max(0.0) as f32;
                    let (count, speed, lifetime, size) = if effect.death {
                        (12, 1.8, 0.8, 0.11)
                    } else {
                        (5, 0.8, 0.42, 0.07)
                    };
                    for index in 0..count {
                        let angle = index as f32 * std::f32::consts::TAU / count as f32;
                        let radius = 0.12 + age * speed;
                        let position = effect.position
                            + Vec3::new(
                                angle.cos() * radius,
                                (index % 3) as f32 * 0.12 + age * speed - 3.0 * age * age,
                                angle.sin() * radius,
                            );
                        let size = size * (1.0 - age / lifetime).max(0.2);
                        let model =
                            Mat4::from_translation(position) * Mat4::from_scale(Vec3::splat(size));
                        gl.uniform_matrix_4_f32_slice(
                            Some(&self.transform),
                            false,
                            &(vp * model).to_cols_array(),
                        );
                        gl.vertex_attrib_4_f32(
                            3,
                            0.55 + (index % 3) as f32 * 0.1,
                            0.02,
                            0.025,
                            1.0,
                        );
                        gl.draw_elements(glow::TRIANGLES, 36, glow::UNSIGNED_SHORT, 0);
                    }
                }
            }

            let mut players = self
                .remote
                .values()
                .filter(|player| player.health > 0)
                .collect::<Vec<_>>();
            players.sort_by(|a, b| {
                let da = (a.x - self.position.x).powi(2) + (a.z - self.position.z).powi(2);
                let db = (b.x - self.position.x).powi(2) + (b.z - self.position.z).powi(2);
                db.total_cmp(&da)
            });
            gl.depth_mask(false);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.sprite_program));
            for player in players {
                let (texture, vaos, views) = match player.team {
                    Team::Boys => (
                        self.operative_texture,
                        &self.operative_vaos,
                        &OPERATIVE_VIEWS,
                    ),
                    Team::Girls => (self.girl_texture, &self.girl_vaos, &GIRL_VIEWS),
                };
                let view_index = operative_view(player, self.position);
                let crop = views[view_index];
                let half_width = 0.95 * (crop[2] - crop[0]) / (crop[3] - crop[1]);
                let facing_viewer = (self.position.x - player.x).atan2(self.position.z - player.z);
                let model = Mat4::from_translation(Vec3::new(player.x, player.y + 0.95, player.z))
                    * Mat4::from_rotation_y(facing_viewer)
                    * Mat4::from_scale(Vec3::new(half_width, 0.95, 1.0));
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.bind_vertex_array(Some(vaos[view_index]));
                gl.uniform_matrix_4_f32_slice(
                    Some(&self.sprite_transform),
                    false,
                    &(vp * model).to_cols_array(),
                );
                gl.draw_arrays(glow::TRIANGLES, 0, 6);
            }
            gl.disable(glow::BLEND);
            gl.depth_mask(true);

            gl.disable(glow::DEPTH_TEST);
            if self.show_blood && now - self.local_hit_at < 350.0 {
                let fade = (1.0 - (now - self.local_hit_at) as f32 / 350.0).clamp(0.0, 1.0);
                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                gl.use_program(Some(self.program));
                gl.bind_vertex_array(Some(self.vao));
                for (x, y, size) in [
                    (-0.91, 0.55, 0.09),
                    (-0.88, -0.55, 0.06),
                    (0.92, 0.35, 0.08),
                    (0.84, -0.72, 0.07),
                ] {
                    let size = size * if self.health == 0 { 1.8 } else { 1.0 };
                    let model = Mat4::from_translation(Vec3::new(x, y, 0.0))
                        * Mat4::from_scale(Vec3::new(size, size * 1.4, 0.01));
                    gl.uniform_matrix_4_f32_slice(
                        Some(&self.transform),
                        false,
                        &model.to_cols_array(),
                    );
                    gl.vertex_attrib_4_f32(3, 0.7, 0.0, 0.02, fade * 0.6);
                    gl.draw_elements(glow::TRIANGLES, 36, glow::UNSIGNED_SHORT, 0);
                }
                gl.disable(glow::BLEND);
            }
            let frame = if now - self.shot_at < 120.0 {
                1
            } else if now - self.shot_at < 240.0 {
                2
            } else {
                0
            };
            let (aspect, sight_x) = match frame {
                0 => (1.5, 768.0 / 1536.0),
                1 => (1.0, 630.0 / 1254.0),
                _ => (1.0, 665.0 / 1254.0),
            };
            let sy = [0.65, 0.69, 0.66][frame];
            let sx = sy * aspect * height as f32 / width as f32;
            let weapon =
                Mat4::from_translation(Vec3::new((0.5 - sight_x) * 2.0 * sx, -1.0 + sy, 0.0))
                    * Mat4::from_scale(Vec3::new(sx, sy, 1.0));
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.sprite_program));
            gl.bind_vertex_array(Some(self.sprite_vao));
            gl.bind_texture(glow::TEXTURE_2D, Some(self.textures[frame]));
            gl.uniform_matrix_4_f32_slice(
                Some(&self.sprite_transform),
                false,
                &weapon.to_cols_array(),
            );
            gl.draw_arrays(glow::TRIANGLES, 0, 6);
            gl.disable(glow::BLEND);
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
        true
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let before = self.settings_values();
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(30, 9, 5, 235))
            .stroke(Stroke::new(1.0, Color32::from_rgb(0x7a, 0x24, 0x12)))
            .inner_margin(egui::Margin::symmetric(22, 18))
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(296.0);
                    ui.spacing_mut().slider_width = 296.0;
                    ui.style_mut().override_font_id = Some(FontId::proportional(18.0));
                    ui.label(format!("Hassasiyet: {}%", self.sensitivity));
                    ui.add(
                        egui::Slider::new(&mut self.sensitivity, 0.0..=200.0)
                            .step_by(1.0)
                            .show_value(false)
                            .trailing_fill(true),
                    );
                    ui.label(format!("Arayüz ölçeği: {}%", self.hud_scale));
                    ui.add(
                        egui::Slider::new(&mut self.hud_scale, 50.0..=200.0)
                            .step_by(10.0)
                            .show_value(false)
                            .trailing_fill(true),
                    );
                    ui.checkbox(&mut self.show_fps, "FPS göster");
                    if ui.checkbox(&mut self.show_blood, "Kan efektleri").changed()
                        && !self.show_blood
                    {
                        self.blood.clear();
                        self.local_hit_at = f64::NEG_INFINITY;
                    }
                    let display = ui.checkbox(&mut self.fullscreen, "Tam ekran").changed();
                    let monitor = ui.ctx().input(|i| i.viewport().monitor_size).map(|size| {
                        size * ui
                            .ctx()
                            .input(|i| i.viewport().native_pixels_per_point.unwrap_or(1.0))
                    });
                    let name = |resolution: Option<[u32; 2]>| match resolution {
                        Some([width, height]) => format!("{width} × {height}"),
                        None => "Ekrana uyarla".to_owned(),
                    };
                    ui.label("Çözünürlük");
                    egui::ComboBox::from_id_salt("resolution")
                        .selected_text(name(self.resolution))
                        .width(296.0)
                        .show_ui(ui, |ui| {
                            for resolution in std::iter::once(None).chain(
                                RESOLUTIONS
                                    .into_iter()
                                    .filter(|[width, height]| {
                                        monitor.is_none_or(|monitor| {
                                            *width as f32 <= monitor.x + 0.5
                                                && *height as f32 <= monitor.y + 0.5
                                        })
                                    })
                                    .map(Some),
                            ) {
                                ui.selectable_value(
                                    &mut self.resolution,
                                    resolution,
                                    name(resolution),
                                )
                                .changed();
                            }
                        });
                    let limit = |fps: u32| match fps {
                        0 => "Sınırsız".to_owned(),
                        fps => format!("{fps} FPS"),
                    };
                    ui.label("FPS sınırı");
                    egui::ComboBox::from_id_salt("fps-limit")
                        .selected_text(limit(self.fps_limit))
                        .width(296.0)
                        .show_ui(ui, |ui| {
                            for fps in FPS_LIMITS {
                                ui.selectable_value(&mut self.fps_limit, fps, limit(fps));
                            }
                        });
                    if display {
                        self.apply_display();
                    }
                })
            });
        if before != self.settings_values() {
            self.save_settings();
        }
    }

    fn apply_display(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if self.fullscreen {
            if web::canvas().request_fullscreen().is_err() {
                self.status = "Tam ekran açılamadı. Tarayıcı izinlerini kontrol edin.".to_owned();
            }
        } else {
            web::document().exit_fullscreen();
        }
    }

    fn settings_values(&self) -> [String; 7] {
        [
            self.sensitivity.to_string(),
            self.hud_scale.to_string(),
            self.show_fps.to_string(),
            self.show_blood.to_string(),
            self.resolution
                .map(|[w, h]| format!("{w},{h}"))
                .unwrap_or_default(),
            self.fps_limit.to_string(),
            format!(
                "{},{},{}",
                self.crosshair.0, self.crosshair.1, self.crosshair.2
            ),
        ]
    }

    fn save_settings(&self) {
        for (key, value) in [
            "sensitivity",
            "hud_scale",
            "show_fps",
            "show_blood",
            "resolution",
            "fps_limit",
            "crosshair",
        ]
        .into_iter()
        .zip(self.settings_values())
        {
            web::storage_set(&format!("keles.{key}"), &value);
        }
    }

    fn load_settings(&mut self) {
        let number = |key: &str| {
            web::storage_get(&format!("keles.{key}"))
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|v| v.is_finite())
        };
        if let Some(v) = number("sensitivity") {
            self.sensitivity = v.clamp(0.0, 200.0);
        }
        if let Some(v) = number("hud_scale") {
            self.hud_scale = v.clamp(50.0, 200.0);
        }
        if let Some(v) = web::storage_get("keles.show_fps").and_then(|v| v.parse().ok()) {
            self.show_fps = v;
        }
        if let Some(v) = web::storage_get("keles.show_blood").and_then(|v| v.parse().ok()) {
            self.show_blood = v;
        }
        if let Some(v) = web::storage_get("keles.fps_limit")
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| FPS_LIMITS.contains(v))
        {
            self.fps_limit = v;
        }
        if let Some(v) = web::storage_get("keles.resolution") {
            if let Some((w, h)) = v.split_once(',') {
                if let (Ok(w), Ok(h)) = (w.parse(), h.parse()) {
                    if RESOLUTIONS.contains(&[w, h]) {
                        self.resolution = Some([w, h]);
                    }
                }
            }
        }
        if let Some(v) = web::storage_get("keles.crosshair") {
            let fields: Vec<_> = v.split(',').collect();
            if fields.len() == 3 {
                if let (Ok(style), Ok(size), Ok(color)) = (
                    fields[0].parse::<usize>(),
                    fields[1].parse::<f32>(),
                    fields[2].parse::<usize>(),
                ) {
                    if style < 3 && size.is_finite() && (3.0..=20.0).contains(&size) && color < 5 {
                        self.crosshair = (style, size, color);
                    }
                }
            }
        }
    }

    fn menu(&mut self, ctx: &egui::Context) {
        if self.screen == Screen::Playing {
            return;
        }
        egui::Area::new(Id::new("menu"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_black_alpha(230))
                    .inner_margin(24)
                    .show(ui, |ui| {
                        ui.set_width(420.0);
                        ui.vertical_centered(|ui| {
                            match self.screen {
                                Screen::Boot => {
                                    ui.label("Giriş yapılıyor…");
                                    if ui.button("Geri").clicked() {
                                        self.logout();
                                    }
                                }
                                Screen::Auth => {
                                    ui.horizontal(|ui| {
                                        ui.selectable_value(
                                            &mut self.register_tab,
                                            false,
                                            "Giriş Yap",
                                        );
                                        ui.selectable_value(
                                            &mut self.register_tab,
                                            true,
                                            "Kayıt Ol",
                                        );
                                    });
                                    ui.add_enabled_ui(self.pending.is_none(), |ui| {
                                        ui.label("Kullanıcı adı");
                                        ui.text_edit_singleline(&mut self.username);
                                        ui.label("Şifre");
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.password)
                                                .password(true),
                                        );
                                        if self.register_tab {
                                            ui.label("Şifre tekrar");
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.confirm)
                                                    .password(true),
                                            );
                                        }
                                        ui.checkbox(&mut self.remember, "Beni hatırla");
                                        if ui
                                            .button(if self.register_tab {
                                                "Kayıt ol"
                                            } else {
                                                "Giriş yap"
                                            })
                                            .clicked()
                                            || ui.input(|i| i.key_pressed(Key::Enter))
                                        {
                                            self.submit_auth();
                                        }
                                    });
                                    if let Some((_, error)) = &self.field_error {
                                        ui.label(error);
                                    }
                                }
                                Screen::Lobby => {
                                    ui.heading(format!("Hoş geldin, {}", self.username));
                                    ui.label(format!(
                                        "{} çevrimiçi · {}/16 odada",
                                        self.online, self.dm_players
                                    ));
                                    if ui
                                        .add_enabled(
                                            self.pending.is_none(),
                                            egui::Button::new("Takım Ölüm Maçı · Oyna"),
                                        )
                                        .clicked()
                                    {
                                        self.mode = Mode::Deathmatch;
                                        self.clear_match();
                                        self.lock_pointer();
                                        self.request("PLAY");
                                    }
                                    if ui
                                        .add_enabled(
                                            self.pending.is_none(),
                                            egui::Button::new("Dereceli maç bul"),
                                        )
                                        .clicked()
                                    {
                                        self.request("QUEUE");
                                    }
                                    if ui.button("Ayarlar").clicked() {
                                        self.settings_back = Screen::Lobby;
                                        self.show_screen(Screen::Settings);
                                    }
                                    if ui.button("Çıkış yap").clicked() {
                                        self.logout();
                                    }
                                }
                                Screen::Queue => {
                                    ui.heading("Rakip aranıyor…");
                                    ui.label(format!(
                                        "{} saniye",
                                        ((self.last_frame - self.queue_since) / 1000.0) as u32
                                    ));
                                    if ui.button("İptal (Esc)").clicked() {
                                        self.request("UNQUEUE");
                                    }
                                }
                                Screen::MatchIntro => {
                                    ui.heading(format!(
                                        "{} — {}",
                                        self.username,
                                        self.opponent
                                            .as_ref()
                                            .map(|o| o.0.as_str())
                                            .unwrap_or("Rakip")
                                    ));
                                    ui.label(format!(
                                        "{:.0}",
                                        ((self.go_at - self.last_frame) / 1000.0).ceil().max(0.0)
                                    ));
                                    if ui.button("Başlamak için tıkla").clicked() {
                                        self.lock_pointer();
                                    }
                                    if ui.button("Lobiye dön").clicked() {
                                        self.lobby();
                                    }
                                }
                                Screen::Paused => {
                                    ui.heading("Duraklatıldı");
                                    if ui.button("Devam et").clicked() {
                                        self.escape();
                                    }
                                    if ui.button("Ayarlar").clicked() {
                                        self.settings_back = Screen::Paused;
                                        self.show_screen(Screen::Settings);
                                    }
                                    if self.confirm_leave {
                                        ui.label("Ayrılmak mağlubiyet sayılır");
                                        if ui.button("Ayrılmayı onayla").clicked() {
                                            self.confirm_leave = false;
                                            self.lobby();
                                        }
                                    } else if ui.button("Maçtan ayrıl").clicked() {
                                        if self.mode == Mode::Ranked && self.ranked_started {
                                            self.confirm_leave = true;
                                        } else {
                                            self.lobby();
                                        }
                                    }
                                }
                                Screen::Settings => {
                                    self.settings(ui);
                                    if ui.button("Geri").clicked() {
                                        self.show_screen(self.settings_back);
                                    }
                                }
                                Screen::Dead => {
                                    ui.heading("Öldün");
                                    ui.label(format!(
                                        "Seni öldürdü: {}",
                                        self.killed_by.as_deref().unwrap_or("Rakip")
                                    ));
                                    if self.mode == Mode::Ranked {
                                        ui.label(format!(
                                            "Yeniden doğuş: {:.0}",
                                            ((self.died_at + 2000.0 - self.last_frame) / 1000.0)
                                                .ceil()
                                                .max(0.0)
                                        ));
                                    } else if ui.button("Yeniden doğ").clicked() {
                                        self.lock_pointer();
                                        self.request("RESPAWN");
                                    }
                                    if ui.button("Lobiye dön").clicked() {
                                        self.lobby();
                                    }
                                }
                                Screen::Result => {
                                    ui.heading(if self.team == self.winner {
                                        "ZAFER"
                                    } else {
                                        "YENİLGİ"
                                    });
                                    ui.label(format!("{} — {}", self.score[0], self.score[1]));
                                    if ui.button("Lobiye dön").clicked() {
                                        self.lobby();
                                    }
                                }
                                Screen::RankedResult => {
                                    if let Some(r) = &self.ranked_result {
                                        ui.heading(match r.result.as_str() {
                                            "win" => "ZAFER",
                                            "loss" => "YENİLGİ",
                                            _ => "BERABERE",
                                        });
                                        ui.label(format!("{} ({:+})", r.rating, r.delta));
                                    }
                                    if ui.button("Tekrar sıraya gir").clicked() {
                                        self.request("QUEUE");
                                    }
                                    if ui.button("Lobiye dön").clicked() {
                                        self.show_screen(Screen::Lobby);
                                        self.send("INFO".to_owned());
                                    }
                                }
                                Screen::Disconnected => {
                                    ui.heading("Bağlantı kesildi");
                                    if ui.button("Yeniden bağlan").clicked() {
                                        if self.token.is_empty() {
                                            self.show_screen(Screen::Auth);
                                        } else if let Err(e) =
                                            self.connect(format!("TOKEN|{}", self.token), true)
                                        {
                                            self.status = e;
                                        }
                                    }
                                    if ui.button("Çıkış").clicked() {
                                        self.logout();
                                    }
                                }
                                Screen::Playing => {}
                            }
                            if self.pending.is_some() {
                                ui.spinner();
                            }
                            ui.label(&self.status);
                        });
                    });
            });
    }

    fn hud(&self, ctx: &egui::Context) {
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("hud")));
        let screen = ctx.viewport_rect();
        let scale = self.hud_scale / 100.0;
        let text = |pos, anchor, text: &str, size, color| {
            let font = FontId::proportional(size);
            painter.text(
                pos + vec2(1.0, 1.0),
                anchor,
                text,
                font.clone(),
                Color32::BLACK,
            );
            painter.text(pos, anchor, text, font, color);
        };
        let in_match = matches!(
            self.screen,
            Screen::Playing | Screen::Paused | Screen::Settings
        );
        if self.socket.is_some() && (in_match || self.screen == Screen::Dead) {
            let mut names = self
                .remote
                .values()
                .map(|player| player.name.as_str())
                .collect::<Vec<_>>();
            names.sort_unstable();
            let status = if !self.connected {
                "Bağlanılıyor…".to_owned()
            } else if self.health == 0 {
                "Öldün\nYeniden doğ düğmesine tıkla".to_owned()
            } else {
                format!(
                    "Sen: {}\nDiğerleri: {}",
                    self.username.trim(),
                    if names.is_empty() {
                        "yok".to_owned()
                    } else {
                        names.join(", ")
                    }
                )
            };
            text(
                screen.min + vec2(12.0, 12.0),
                Align2::LEFT_TOP,
                &status,
                16.0 * scale,
                TEXT,
            );
        }
        if self.show_fps && in_match {
            let pos = pos2(screen.right() - 12.0, screen.top() + 12.0);
            text(
                pos,
                Align2::RIGHT_TOP,
                &format!("FPS: {}", self.fps),
                16.0 * scale,
                TEXT,
            );
        }
        if in_match || self.screen == Screen::Dead {
            let bar = Rect::from_min_size(
                pos2(screen.left() + 18.0, screen.bottom() - 18.0 - 14.0 * scale),
                vec2(180.0, 14.0) * scale,
            );
            painter.rect_filled(bar, 0.0, Color32::from_rgb(0x1c, 0x08, 0x06));
            let mut fill = bar;
            fill.set_width(bar.width() * self.health as f32 / 3.0);
            painter.rect_filled(fill, 0.0, Color32::from_rgb(0xff, 0x6a, 0x22));
            painter.rect_stroke(bar, 0.0, Stroke::new(1.0, CRIMSON), StrokeKind::Outside);
            let label = format!("Can {}/3", self.health);
            text(
                bar.left_top() - vec2(0.0, 5.0 * scale),
                Align2::LEFT_BOTTOM,
                &label,
                18.0 * scale,
                TEXT,
            );
        }
        if self.socket.is_some()
            && (in_match || matches!(self.screen, Screen::Dead | Screen::Result))
        {
            // Each team's kills around the kill target, with the local team marked.
            let center = pos2(screen.center().x, screen.top() + 43.0);
            let panel = Rect::from_center_size(center, vec2(330.0, 62.0));
            painter.rect_filled(panel, 0.0, Color32::from_rgba_unmultiplied(30, 9, 5, 220));
            painter.rect_stroke(panel, 0.0, Stroke::new(1.0, CRIMSON), StrokeKind::Inside);
            for (dx, team, score) in [
                (-105.0, Team::Boys, self.score[0]),
                (105.0, Team::Girls, self.score[1]),
            ] {
                let you = if self.team == Some(team) {
                    " · SEN"
                } else {
                    ""
                };
                let label = format!("{}{you}", team.label().to_uppercase());
                text(
                    center + vec2(dx, -17.0),
                    Align2::CENTER_CENTER,
                    &label,
                    12.0,
                    team.color(),
                );
                text(
                    center + vec2(dx, 8.0),
                    Align2::CENTER_CENTER,
                    &score.to_string(),
                    34.0,
                    team.color(),
                );
            }
            let gray = Color32::from_rgb(0xc8, 0xcc, 0xd4);
            text(
                center + vec2(0.0, -12.0),
                Align2::CENTER_CENTER,
                "HEDEF",
                11.0,
                gray,
            );
            text(
                center + vec2(0.0, 9.0),
                Align2::CENTER_CENTER,
                &map::WIN_SCORE.to_string(),
                20.0,
                gray,
            );
        }
    }
}

impl eframe::App for Game {
    #[cfg(target_arch = "wasm32")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time) * 1000.0;
        if now - self.fps_since >= 500.0 {
            self.fps = (self.fps_frames as f64 * 1000.0 / (now - self.fps_since)).round() as u32;
            self.fps_since = now;
            self.fps_frames = 0;
        }
        self.fps_frames += 1;
        // Start after WebRunner initialization, which can outlast the server's auth deadline.
        if self.screen == Screen::Boot && self.socket.is_none() && self.pending.is_none() {
            self.last_frame = now;
            if let Err(error) = self.connect(format!("TOKEN|{}", self.token), true) {
                self.status = error;
                self.show_screen(Screen::Auth);
            }
        }
        self.poll_socket();
        if self.pending.is_some_and(|(_, deadline)| now >= deadline) {
            self.pending = None;
            self.status = "İşlem zaman aşımına uğradı. Yeniden deneyin.".to_owned();
            self.toasts.push((self.status.clone(), now, true));
            self.disconnect();
            self.show_screen(if self.account.is_some() {
                Screen::Disconnected
            } else {
                Screen::Auth
            });
        }
        if matches!(self.screen, Screen::Lobby | Screen::Queue) && now - self.last_info >= 5000.0 {
            self.send("INFO".to_owned());
            self.last_info = now;
        }
        if self.screen == Screen::MatchIntro && self.ranked_started && self.grabbed {
            self.show_screen(Screen::Playing);
        }
        if self.mode == Mode::Ranked
            && self.health == 0
            && self.connected
            && self.winner.is_none()
            && matches!(self.screen, Screen::Dead | Screen::Paused)
            && now - self.died_at >= 2000.0
            && self.pending.is_none()
        {
            self.request("RESPAWN");
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.grabbed = web::document().pointer_lock_element().is_some();
            self.fullscreen = web::document().fullscreen_element().is_some();
        }
        self.input(&ctx);
        if self.screen == Screen::Result {
            let remaining = ((self.result_deadline - now) / 1000.0).ceil().max(0.0) as u32;
            self.status = format!("Yeniden başlıyor: {remaining}");
        }

        let screen = ctx.viewport_rect();
        let window = (screen.size() * ctx.pixels_per_point()).round();
        let target = match self.resolution {
            Some(resolution) => resolution,
            _ => [window.x.max(1.0) as u32, window.y.max(1.0) as u32],
        };
        if self.draw(now, target) {
            let (program, vao, texture) = (self.sprite_program, self.sprite_vao, self.color);
            // Adapted from emilk/egui, crates/egui_demo_app/src/apps/custom3d_glow.rs,
            // lines 67-75, commit 2ec7a836879b958f05ef542fab5a12a86250ef62.
            // https://github.com/emilk/egui/blob/2ec7a836879b958f05ef542fab5a12a86250ef62/crates/egui_demo_app/src/apps/custom3d_glow.rs#L67
            // License: MIT (see LICENSE-MIT-egui). Stretches the scene over the window.
            let callback = egui_glow::CallbackFn::new(move |_info, painter| unsafe {
                let gl = painter.gl();
                // Rows of a framebuffer texture start at the bottom.
                let flip = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0)).to_cols_array();
                gl.disable(glow::BLEND);
                gl.use_program(Some(program));
                let transform = gl.get_uniform_location(program, "MVP");
                gl.uniform_matrix_4_f32_slice(transform.as_ref(), false, &flip);
                gl.bind_vertex_array(Some(vao));
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                gl.draw_arrays(glow::TRIANGLES, 0, 6);
            });
            ctx.layer_painter(LayerId::background())
                .add(egui::PaintCallback {
                    rect: screen,
                    callback: Arc::new(callback),
                });
        }
        self.menu(&ctx);
        self.hud(&ctx);
        // eframe schedules this through requestAnimationFrame; never block the browser.
        if self.fps_limit == 0 {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                1.0 / self.fps_limit as f64,
            ));
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.08, 0.11, 0.16, 1.0]
    }
}

fn floats(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_ne_bytes())
        .collect()
}

fn decode(png: &[u8]) -> ([usize; 2], Vec<u8>) {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .expect("Embedded PNG is invalid")
        .to_rgba8();
    (
        [image.width() as usize, image.height() as usize],
        image.into_raw(),
    )
}

unsafe fn texture(gl: &glow::Context, png: &[u8], wrap: u32) -> glow::Texture {
    let ([width, height], rgba) = decode(png);
    let texture = gl.create_texture().expect("No texture");
    gl.bind_texture(glow::TEXTURE_2D, Some(texture));
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MIN_FILTER,
        glow::LINEAR as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MAG_FILTER,
        glow::LINEAR as i32,
    );
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, wrap as i32);
    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, wrap as i32);
    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::RGBA as i32,
        width as i32,
        height as i32,
        0,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelUnpackData::Slice(Some(&rgba)),
    );
    texture
}

// One interleaved float buffer; attributes are (location, size, byte offset).
unsafe fn vertex_array(
    gl: &glow::Context,
    data: &[u8],
    stride: i32,
    attributes: &[(u32, i32, i32)],
) -> glow::VertexArray {
    let vao = gl.create_vertex_array().expect("No vertex array");
    gl.bind_vertex_array(Some(vao));
    let buffer = gl.create_buffer().expect("No vertex buffer");
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, data, glow::STATIC_DRAW);
    for &(location, size, offset) in attributes {
        gl.vertex_attrib_pointer_f32(location, size, glow::FLOAT, false, stride, offset);
        gl.enable_vertex_attrib_array(location);
    }
    vao
}

unsafe fn operative_vaos(
    gl: &glow::Context,
    views: &[[f32; 4]; 4],
    width: f32,
    height: f32,
) -> Vec<glow::VertexArray> {
    views
        .iter()
        .map(|&[left, top, right, bottom]| {
            let (u0, v0, u1, v1) = (left / width, top / height, right / width, bottom / height);
            let quad = [
                -1.0, -1.0, u0, v1, 1.0, -1.0, u1, v1, 1.0, 1.0, u1, v0, -1.0, -1.0, u0, v1, 1.0,
                1.0, u1, v0, -1.0, 1.0, u0, v0,
            ];
            vertex_array(gl, &floats(&quad), 16, &[(0, 2, 0), (4, 2, 8)])
        })
        .collect()
}

// Adapted from wasm-bindgen/examples/webgl/src/lib.rs, lines 89-134.
// https://github.com/wasm-bindgen/wasm-bindgen/blob/main/examples/webgl/src/lib.rs
// License: MIT (see LICENSE-MIT-wasm-bindgen). Keeps compile and link error logs visible.
unsafe fn link_program(gl: &glow::Context, vertex: &str, fragment: &str) -> glow::Program {
    let program = gl.create_program().expect("Could not create program");
    for (kind, source) in [
        (glow::VERTEX_SHADER, vertex),
        (glow::FRAGMENT_SHADER, fragment),
    ] {
        // Desktop OpenGL takes the same shaders with a desktop GLSL header.
        let source = if gl.version().is_embedded {
            source.to_owned()
        } else {
            source.replacen("#version 300 es", "#version 330 core", 1)
        };
        let shader = gl.create_shader(kind).expect("Could not create shader");
        gl.shader_source(shader, &source);
        gl.compile_shader(shader);
        assert!(
            gl.get_shader_compile_status(shader),
            "{}",
            gl.get_shader_info_log(shader)
        );
        gl.attach_shader(program, shader);
        gl.delete_shader(shader);
    }
    gl.link_program(program);
    assert!(
        gl.get_program_link_status(program),
        "{}",
        gl.get_program_info_log(program)
    );
    program
}
