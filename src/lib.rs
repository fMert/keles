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
use glam::{camera, Mat4, Quat, Vec3};
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
    pub fn sfx_fire(_: f32, _: f32) {}
    pub fn sfx_hit(_: f32, _: bool, _: u32) {}
    pub fn sfx_hurt(_: f32) {}
    pub fn sfx_step(_: f32, _: bool) {}
    pub fn sfx_tick(_: f32, _: bool) {}
    pub fn sfx_end(_: f32, _: bool) {}
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

// shortcut: the owner keeps auth messages at 256 bytes; widen that limit before supporting every 64-character Unicode password.
fn auth_fits(name: &str, password: &str, register: bool) -> bool {
    name.len() + password.len() + if register { 10 } else { 7 } <= 256
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
        assert!(super::auth_fits("UserA", &"ş".repeat(64), true));
        assert!(!super::auth_fits("UserA", &"😀".repeat(64), true));
        assert_eq!(
            super::auth_error("wrong"),
            "Kullanıcı adı veya şifre hatalı."
        );
    }
}

// GLSL lines from WebGLSamples/WebGL2Samples/samples/transform_feedback_interleaved.html,
// lines 20-33 and 52-84; extended with world-space directional sunlight, muzzle light, and fog.
// https://github.com/WebGLSamples/WebGL2Samples/blob/master/samples/transform_feedback_interleaved.html
// License: MIT (see LICENSE-MIT-WebGL2Samples).
const VERTEX_SHADER: &str = r#"#version 300 es
#define POSITION_LOCATION 0
#define NORMAL_LOCATION 1
#define COLOR_POSITION 3
precision highp float;
precision highp int;
uniform mat4 MVP;
uniform mat4 MODEL;
layout(location = POSITION_LOCATION) in vec4 position;
layout(location = NORMAL_LOCATION) in vec3 normal;
layout(location = COLOR_POSITION) in vec4 color;
layout(location = 4) in vec2 texcoord;
out vec4 v_color;
out vec2 v_uv;
out vec3 v_world;
out vec3 v_normal;
void main()
{
    gl_Position = MVP * position;
    v_color = color;
    v_uv = texcoord;
    v_world = (MODEL * position).xyz;
    v_normal = mat3(MODEL) * normal;
}
"#;
const FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;
precision highp int;
in vec4 v_color;
in vec2 v_uv;
in vec3 v_world;
in vec3 v_normal;
uniform sampler2D diffuse;
uniform bool use_texture;
uniform bool use_vertex_normal;
uniform bool unlit;
uniform bool no_fog;
uniform vec4 hit_flash;
uniform vec3 cam_pos;
uniform vec3 muzzle_pos;
uniform float muzzle_light;
out vec4 color;
void main()
{
    vec4 base = v_color;
    if (use_texture) base.rgb *= texture(diffuse, v_uv).rgb;
    if (unlit) {
        color = vec4(mix(base.rgb, hit_flash.rgb, hit_flash.a), base.a);
        return;
    }
    vec3 n = use_vertex_normal
        ? normalize(v_normal)
        : normalize(cross(dFdx(v_world), dFdy(v_world)));
    if (!gl_FrontFacing) n = -n;
    vec3 sun_dir = normalize(vec3(0.58, 0.56, -0.42));
    float ndl = max(dot(n, sun_dir), 0.0);
    vec3 hemi = mix(vec3(0.46, 0.37, 0.27), vec3(0.58, 0.68, 0.80), n.y * 0.5 + 0.5);
    vec3 lit = base.rgb * (hemi * 0.72 + vec3(1.12, 0.99, 0.82) * (0.20 + 0.62 * ndl));
    if (use_vertex_normal) {
        vec3 view_dir = normalize(cam_pos - v_world);
        vec3 half_vec = normalize(sun_dir + view_dir);
        float spec = pow(max(dot(n, half_vec), 0.0), 26.0) * 0.22;
        float rim = pow(1.0 - max(dot(n, view_dir), 0.0), 3.0) * 0.10;
        lit += vec3(1.0, 0.94, 0.84) * spec + vec3(0.86, 0.78, 0.68) * rim * base.rgb;
    }
    if (muzzle_light > 0.001) {
        float d = length(v_world - muzzle_pos);
        float att = muzzle_light / (1.0 + d * d * 0.32);
        lit += base.rgb * vec3(1.45, 0.68, 0.22) * att;
    }
    if (!no_fog) {
        float dist = length(v_world - cam_pos);
        float fog = clamp((dist - 30.0) / 88.0, 0.0, 0.54);
        lit = mix(lit, vec3(0.89, 0.79, 0.65), fog);
    }
    lit = mix(lit, hit_flash.rgb, hit_flash.a);
    color = vec4(lit, base.a);
}
"#;

const SKY_VERTEX_SHADER: &str = r#"#version 300 es
#define POSITION_LOCATION 0
precision highp float;
layout(location = POSITION_LOCATION) in vec2 position;
out vec2 v_ndc;
void main()
{
    v_ndc = position;
    gl_Position = vec4(position, 0.999, 1.0);
}
"#;
const SKY_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;
in vec2 v_ndc;
uniform vec3 cam_fwd;
uniform vec3 cam_right;
uniform vec3 cam_up;
uniform float aspect;
uniform float tan_half_fov;
out vec4 color;
void main()
{
    vec3 dir = normalize(cam_fwd + v_ndc.x * aspect * tan_half_fov * cam_right + v_ndc.y * tan_half_fov * cam_up);
    float h = clamp(dir.y, -0.25, 1.0);
    vec3 horizon = vec3(0.93, 0.80, 0.63);
    vec3 mid_sky = vec3(0.52, 0.72, 0.88);
    vec3 zenith  = vec3(0.20, 0.43, 0.73);
    vec3 sky = mix(horizon, mid_sky, smoothstep(-0.04, 0.28, h));
    sky = mix(sky, zenith, smoothstep(0.24, 0.82, h));
    vec3 sun_dir = normalize(vec3(0.58, 0.56, -0.42));
    float sun = max(dot(dir, sun_dir), 0.0);
    sky += vec3(1.0, 0.72, 0.36) * pow(sun, 10.0) * 0.38;
    sky += vec3(1.0, 0.95, 0.80) * smoothstep(0.9975, 0.9994, sun) * 0.9;
    float haze = smoothstep(0.18, -0.05, h);
    sky = mix(sky, vec3(0.87, 0.75, 0.59), haze * 0.45);
    color = vec4(sky, 1.0);
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
uniform vec4 tint;
uniform vec4 flash;
in vec2 v_st;
out vec4 color;
void main()
{
    vec4 tex = texture(diffuse, v_st);
    vec3 rgb = mix(tex.rgb * tint.rgb, flash.rgb, flash.a);
    color = vec4(rgb, tex.a * tint.a);
}
"#;

mod ui;

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

// Original DE_KERVAN textures extracted from assets/kervan_original.glb (GPL-3.0-or-later).
macro_rules! kervan_images {
    ($($index:literal)*) => {
        [$(include_bytes!(concat!("../assets/kervan_images/", $index, ".png")).as_slice()),*]
    };
}
const KERVAN_IMAGES: [&[u8]; 29] = kervan_images!(
    0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28
);
macro_rules! model_images {
    ($dir:literal; $($index:literal)*) => {
        [$(include_bytes!(concat!("../assets/", $dir, "/", $index, ".png")).as_slice()),*]
    };
}
const MALE_IMAGES: [&[u8]; 4] = model_images!("male_images"; 0 1 2 3);
const FEMALE_IMAGES: [&[u8]; 2] = model_images!("female_images"; 0 1);
const WEAPON_IMAGES: [&[u8]; 6] = model_images!("weapon_images"; 0 1 2 3 4 5);

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
    Practice,
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
            Team::Boys => ui::BOYS,
            Team::Girls => ui::GIRLS,
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
    hit_at: f64,
    last_shot_at: f64,
    walk_phase: f32,
    move_speed: f32,
    bot_wp: usize,
    bot_fire_at: f64,
    bot_strafe: f32,
    bot_respawn_at: f64,
}

struct BloodEffect {
    position: Vec3,
    at: f64,
    death: bool,
}

struct Tracer {
    start: Vec3,
    end: Vec3,
    at: f64,
    mine: bool,
}

struct Spark {
    position: Vec3,
    velocity: Vec3,
    at: f64,
}

struct KervanPart {
    first: i32,
    count: i32,
    image: usize,
}

struct ModelNode {
    parent: Option<usize>,
    name: String,
    rest_t: Vec3,
    rest_r: Quat,
    rest_s: Vec3,
}

struct ModelPart {
    node_idx: usize,
    first: i32,
    count: i32,
    image: usize,
}

struct ArticulatedModel {
    vao: glow::VertexArray,
    nodes: Vec<ModelNode>,
    parts: Vec<ModelPart>,
    textures: Vec<glow::Texture>,
}

impl ArticulatedModel {
    unsafe fn load(gl: &glow::Context, mesh_bin: &[u8], manifest: &str, images: &[&[u8]]) -> Self {
        let vao = vertex_array(gl, mesh_bin, 32, &[(0, 3, 0), (1, 3, 12), (4, 2, 24)]);
        let mut nodes = Vec::new();
        let mut parts = Vec::new();
        for line in manifest.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            match f.first().copied() {
                Some("N") if f.len() >= 14 => {
                    let parent_raw: isize = f[2].parse().unwrap();
                    nodes.push(ModelNode {
                        parent: (parent_raw >= 0).then_some(parent_raw as usize),
                        name: f[3].to_owned(),
                        rest_t: Vec3::new(
                            f[4].parse().unwrap(),
                            f[5].parse().unwrap(),
                            f[6].parse().unwrap(),
                        ),
                        rest_r: Quat::from_xyzw(
                            f[7].parse().unwrap(),
                            f[8].parse().unwrap(),
                            f[9].parse().unwrap(),
                            f[10].parse().unwrap(),
                        )
                        .normalize(),
                        rest_s: Vec3::new(
                            f[11].parse().unwrap(),
                            f[12].parse().unwrap(),
                            f[13].parse().unwrap(),
                        ),
                    });
                }
                Some("P") if f.len() >= 5 => {
                    parts.push(ModelPart {
                        node_idx: f[1].parse().unwrap(),
                        first: f[2].parse().unwrap(),
                        count: f[3].parse().unwrap(),
                        image: f[4].parse().unwrap(),
                    });
                }
                _ => {}
            }
        }
        let textures = images
            .iter()
            .map(|png| texture(gl, png, glow::REPEAT))
            .collect();
        Self {
            vao,
            nodes,
            parts,
            textures,
        }
    }

    fn node_index(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }

    fn compute_world(&self, root: Mat4, offsets: &[(usize, Vec3, Quat, Vec3)]) -> Vec<Mat4> {
        let mut world = vec![Mat4::IDENTITY; self.nodes.len()];
        for (i, n) in self.nodes.iter().enumerate() {
            let (mut dt, mut dr, mut ds) = (Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
            for &(idx, t, r, s) in offsets {
                if idx == i {
                    dt += t;
                    dr *= r;
                    ds *= s;
                }
            }
            let local =
                Mat4::from_scale_rotation_translation(n.rest_s * ds, n.rest_r * dr, n.rest_t + dt);
            world[i] = match n.parent {
                Some(p) => world[p] * local,
                None => root * local,
            };
        }
        world
    }
}

struct Game {
    ctx: egui::Context,
    gl: Arc<glow::Context>,
    program: glow::Program,
    sky_program: glow::Program,
    vao: glow::VertexArray,
    kervan_vao: glow::VertexArray,
    kervan_parts: Vec<KervanPart>,
    kervan_textures: Vec<glow::Texture>,
    transform: glow::UniformLocation,
    model_loc: glow::UniformLocation,
    texture_enabled: glow::UniformLocation,
    use_vnormal_loc: glow::UniformLocation,
    unlit_loc: glow::UniformLocation,
    no_fog_loc: glow::UniformLocation,
    hit_flash_loc: glow::UniformLocation,
    cam_pos_loc: glow::UniformLocation,
    muzzle_pos_loc: glow::UniformLocation,
    muzzle_light_loc: glow::UniformLocation,
    sky_fwd_loc: glow::UniformLocation,
    sky_right_loc: glow::UniformLocation,
    sky_up_loc: glow::UniformLocation,
    sky_aspect_loc: glow::UniformLocation,
    sky_fov_loc: glow::UniformLocation,
    sprite_program: glow::Program,
    sprite_vao: glow::VertexArray,
    male_model: ArticulatedModel,
    female_model: ArticulatedModel,
    weapon_model: ArticulatedModel,
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
    last_draw: f64,
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
    kill_hit_at: f64,
    killed_by: Option<String>,
    died_at: f64,
    last_respawn: f64,
    kills: u32,
    deaths: u32,
    streak: u32,
    best_streak: u32,
    multi_kill: u32,
    last_kill_at: f64,
    kill_banner: Option<(String, String, f64)>,
    damage_angle: Option<(f32, f64)>,
    last_countdown_sec: u32,
    toasts: Vec<(String, f64, bool)>,
    crosshair: (usize, f32, usize),
    dynamic_crosshair: bool,
    show_radar: bool,
    volume: f32,
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
    backdrop_slow: u8,
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
    tracers: Vec<Tracer>,
    sparks: Vec<Spark>,
    local_hit_at: f64,
    grabbed: bool,
    mouse_delta: Vec2,
    connect_deadline: f64,
    // Camera and weapon feel state.
    recoil_pitch: f32,
    strafe_roll: f32,
    fov_kick: f32,
    weapon_sway: Vec2,
    bob_phase: f32,
    bob_amount: f32,
    land_dip: f32,
    was_airborne: bool,
    step_dist: f32,
    last_vp: Mat4,
    practice_rng: u64,
    practice_back: Screen,
}

impl Game {
    fn new(cc: &eframe::CreationContext) -> Self {
        let gl = cc.gl.clone().expect("Keles needs OpenGL");
        let ctx = &cc.egui_ctx;
        ui::apply_theme(ctx);
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

        let kervan_parts = include_str!("../assets/kervan_parts.txt")
            .lines()
            .map(|line| {
                let mut fields = line.split_whitespace();
                KervanPart {
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
            let sky_program = link_program(&gl, SKY_VERTEX_SHADER, SKY_FRAGMENT_SHADER);
            let sprite_program = link_program(&gl, SPRITE_VERTEX_SHADER, SPRITE_FRAGMENT_SHADER);
            let vao = vertex_array(&gl, &floats(&VERTICES), 0, &[(0, 3, 0)]);
            let indices = gl.create_buffer().expect("No index buffer");
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(indices));
            let index_bytes: Vec<u8> = INDICES.iter().flat_map(|i| i.to_ne_bytes()).collect();
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, &index_bytes, glow::STATIC_DRAW);
            // Original DE_KERVAN map mesh extracted by tools/convert (GPL-3.0-or-later).
            let kervan_vao = vertex_array(
                &gl,
                include_bytes!("../assets/kervan_mesh.bin"),
                20,
                &[(0, 3, 0), (4, 2, 12)],
            );
            let sprite_vao =
                vertex_array(&gl, &floats(&SPRITE_VERTICES), 16, &[(0, 2, 0), (4, 2, 8)]);
            let male_model = ArticulatedModel::load(
                &gl,
                include_bytes!("../assets/male_mesh.bin"),
                include_str!("../assets/male_nodes.txt"),
                &MALE_IMAGES,
            );
            let female_model = ArticulatedModel::load(
                &gl,
                include_bytes!("../assets/female_mesh.bin"),
                include_str!("../assets/female_nodes.txt"),
                &FEMALE_IMAGES,
            );
            let weapon_model = ArticulatedModel::load(
                &gl,
                include_bytes!("../assets/weapon_mesh.bin"),
                include_str!("../assets/weapon_nodes.txt"),
                &WEAPON_IMAGES,
            );
            Game {
                ctx: ctx.clone(),
                transform: gl.get_uniform_location(program, "MVP").unwrap(),
                model_loc: gl.get_uniform_location(program, "MODEL").unwrap(),
                texture_enabled: gl.get_uniform_location(program, "use_texture").unwrap(),
                use_vnormal_loc: gl
                    .get_uniform_location(program, "use_vertex_normal")
                    .unwrap(),
                unlit_loc: gl.get_uniform_location(program, "unlit").unwrap(),
                no_fog_loc: gl.get_uniform_location(program, "no_fog").unwrap(),
                hit_flash_loc: gl.get_uniform_location(program, "hit_flash").unwrap(),
                cam_pos_loc: gl.get_uniform_location(program, "cam_pos").unwrap(),
                muzzle_pos_loc: gl.get_uniform_location(program, "muzzle_pos").unwrap(),
                muzzle_light_loc: gl.get_uniform_location(program, "muzzle_light").unwrap(),
                sky_fwd_loc: gl.get_uniform_location(sky_program, "cam_fwd").unwrap(),
                sky_right_loc: gl.get_uniform_location(sky_program, "cam_right").unwrap(),
                sky_up_loc: gl.get_uniform_location(sky_program, "cam_up").unwrap(),
                sky_aspect_loc: gl.get_uniform_location(sky_program, "aspect").unwrap(),
                sky_fov_loc: gl
                    .get_uniform_location(sky_program, "tan_half_fov")
                    .unwrap(),
                program,
                sky_program,
                vao,
                kervan_vao,
                kervan_parts,
                kervan_textures: KERVAN_IMAGES
                    .iter()
                    .map(|png| texture(&gl, png, glow::REPEAT))
                    .collect(),
                sprite_program,
                sprite_vao,
                male_model,
                female_model,
                weapon_model,
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
                last_draw: 0.0,
                screen: Screen::Auth,
                map: Map::Kervan,
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
                kill_hit_at: f64::NEG_INFINITY,
                killed_by: None,
                died_at: 0.0,
                last_respawn: f64::NEG_INFINITY,
                kills: 0,
                deaths: 0,
                streak: 0,
                best_streak: 0,
                multi_kill: 0,
                last_kill_at: f64::NEG_INFINITY,
                kill_banner: None,
                damage_angle: None,
                last_countdown_sec: 0,
                toasts: Vec::new(),
                crosshair: (0, 7.0, 1),
                dynamic_crosshair: true,
                show_radar: true,
                volume: 70.0,
                confirm_leave: false,
                status: String::new(),
                sensitivity: 50.0,
                hud_scale: 100.0,
                show_fps: true,
                show_blood: true,
                fullscreen: false,
                resolution: None,
                fps_limit: 60,
                fps: 0,
                fps_since: 0.0,
                fps_frames: 0,
                backdrop_slow: 0,
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
                tracers: Vec::new(),
                sparks: Vec::new(),
                local_hit_at: f64::NEG_INFINITY,
                grabbed: false,
                mouse_delta: Vec2::ZERO,
                connect_deadline: 0.0,
                recoil_pitch: 0.0,
                strafe_roll: 0.0,
                fov_kick: 0.0,
                weapon_sway: Vec2::ZERO,
                bob_phase: 0.0,
                bob_amount: 0.0,
                land_dip: 0.0,
                was_airborne: false,
                step_dist: 0.0,
                last_vp: Mat4::IDENTITY,
                practice_rng: 0x517c_c1b7_2722_0a95,
                practice_back: Screen::Auth,
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
        self.tracers.clear();
        self.sparks.clear();
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
            && !(screen == Screen::Dead && self.mode == Mode::Ranked)
            && !(screen == Screen::Boot && self.grabbed)
        {
            web::document().exit_pointer_lock();
        }
        match screen {
            Screen::Playing | Screen::Lobby | Screen::Dead | Screen::RankedResult => {
                self.status.clear()
            }
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
        if self.mode == Mode::Practice {
            let back = if self.account.is_some() {
                Screen::Lobby
            } else {
                self.practice_back
            };
            self.clear_match();
            self.mode = Mode::Deathmatch;
            self.show_screen(back);
            if self.account.is_some() {
                self.send("INFO".to_owned());
            }
            return;
        }
        self.request("LOBBY");
    }

    fn next_rand(&mut self) -> f32 {
        let mut x = self.practice_rng | 1;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.practice_rng = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / ((1u64 << 24) as f32)
    }

    fn spawn_point(&mut self, team: Team) -> Vec3 {
        let (area, floor) = match team {
            Team::Boys => (map::KERVAN_T_SPAWN, map::KERVAN_T_FLOOR),
            Team::Girls => (map::KERVAN_CT_SPAWN, map::KERVAN_CT_FLOOR),
        };
        for _ in 0..24 {
            let x = area[0] + self.next_rand() * (area[2] - area[0]);
            let z = area[1] + self.next_rand() * (area[3] - area[1]);
            if let Some(y) = map::stand_height(Map::Kervan, x, z, floor, 0.28) {
                return Vec3::new(x, y, z);
            }
        }
        Vec3::new((area[0] + area[2]) * 0.5, floor, (area[1] + area[3]) * 0.5)
    }

    fn start_practice(&mut self, back: Screen) {
        self.practice_back = back;
        self.mode = Mode::Practice;
        self.clear_match();
        self.kill_target = map::WIN_SCORE;
        self.self_id = Some(0);
        self.team = Some(Team::Boys);
        self.connected = true;
        self.health = 3;
        if self.username.trim().is_empty() {
            self.username = "Komando".to_owned();
        }
        let spawn = self.spawn_point(Team::Boys);
        self.position = Vec3::new(spawn.x, spawn.y + map::EYE_HEIGHT, spawn.z);
        self.ground = self.position;
        self.vertical = 0.0;
        self.yaw = -std::f32::consts::FRAC_PI_2;
        self.pitch = 0.0;
        let bots = [
            (1, "Kaptan", Team::Boys, 1),
            (2, "Bozkurt", Team::Boys, 26),
            (3, "Demir", Team::Boys, 31),
            (4, "Aylin", Team::Girls, 17),
            (5, "Zeynep", Team::Girls, 19),
            (6, "Ece", Team::Girls, 20),
            (7, "Defne", Team::Girls, 16),
        ];
        for (id, name, team, wp) in bots {
            let pos = self.spawn_point(team);
            let strafe = if id % 2 == 0 { 1.0 } else { -1.0 };
            let delay = 800.0 + self.next_rand() as f64 * 1200.0;
            self.remote.insert(
                id,
                RemotePlayer {
                    name: name.to_owned(),
                    x: pos.x,
                    z: pos.z,
                    y: pos.y,
                    target: pos,
                    yaw: if team == Team::Boys {
                        -std::f32::consts::FRAC_PI_2
                    } else {
                        std::f32::consts::PI
                    },
                    health: 3,
                    team,
                    kills: 0,
                    deaths: 0,
                    hit_at: f64::NEG_INFINITY,
                    last_shot_at: f64::NEG_INFINITY,
                    walk_phase: id as f32 * 1.1,
                    move_speed: 0.0,
                    bot_wp: wp,
                    bot_fire_at: self.last_frame + delay,
                    bot_strafe: strafe,
                    bot_respawn_at: f64::NEG_INFINITY,
                },
            );
        }
        self.lock_pointer();
        self.show_screen(Screen::Playing);
    }

    fn respawn_practice_self(&mut self) {
        let team = self.team.unwrap_or(Team::Boys);
        let spawn = self.spawn_point(team);
        self.position = Vec3::new(spawn.x, spawn.y + map::EYE_HEIGHT, spawn.z);
        self.ground = self.position;
        self.vertical = 0.0;
        self.yaw = if team == Team::Girls {
            std::f32::consts::PI
        } else {
            -std::f32::consts::FRAC_PI_2
        };
        self.pitch = 0.0;
        self.health = 3;
        self.streak = 0;
        self.shot_at = f64::NEG_INFINITY;
        self.winner = None;
        self.lock_pointer();
        self.show_screen(Screen::Playing);
    }

    fn spawn_shot_vfx(&mut self, origin: Vec3, dir: Vec3, dist: f32, mine: bool, hit_wall: bool) {
        let end = origin + dir * dist.min(95.0);
        let start = if mine {
            let right = Vec3::new(-dir.z, 0.0, dir.x).normalize_or_zero();
            origin + dir * 0.75 + right * 0.06 - Vec3::Y * 0.16
        } else {
            origin + dir * 0.55 - Vec3::Y * 0.12
        };
        if self.tracers.len() >= 18 {
            self.tracers.remove(0);
        }
        self.tracers.push(Tracer {
            start,
            end,
            at: self.last_frame,
            mine,
        });
        if hit_wall && dist < 90.0 {
            let base = end - dir * 0.08;
            for i in 0..5 {
                let a = i as f32 * 1.26 + (self.last_frame as f32 * 0.01);
                if self.sparks.len() >= 36 {
                    self.sparks.remove(0);
                }
                self.sparks.push(Spark {
                    position: base,
                    velocity: Vec3::new(
                        a.cos() * 2.2 - dir.x * 1.4,
                        1.4 + (i % 3) as f32 * 0.7,
                        a.sin() * 2.2 - dir.z * 1.4,
                    ),
                    at: self.last_frame,
                });
            }
        }
    }

    fn register_local_kill(&mut self, victim_name: &str) {
        self.streak += 1;
        self.best_streak = self.best_streak.max(self.streak);
        if self.last_frame - self.last_kill_at < 4500.0 {
            self.multi_kill += 1;
        } else {
            self.multi_kill = 1;
        }
        self.last_kill_at = self.last_frame;
        self.kill_hit_at = self.last_frame;
        self.fov_kick = (self.fov_kick + 2.4).min(4.0);
        web::sfx_hit(self.volume, true, self.multi_kill.max(self.streak));
        let headline = match self.multi_kill {
            2 => "ÇİFTE LEŞ!",
            3 => "ÜÇLÜ LEŞ!",
            4 => "DÖRTLÜ LEŞ!",
            n if n >= 5 => "EFSANE SERİ!",
            _ => match self.streak {
                3 => "3 SERİ · KIZIŞTI!",
                5 => "5 SERİ · DURDURULAMAZ!",
                n if n >= 7 => "KRAL SERİ!",
                _ => "+1 LEŞ",
            },
        };
        self.kill_banner = Some((headline.to_owned(), victim_name.to_owned(), self.last_frame));
    }

    fn practice_fire_local(&mut self) {
        let origin = self.position;
        let dir = Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        );
        let mut nearest = map::ray_map(origin.to_array(), dir.to_array());
        let wall_dist = nearest;
        let mut hit = None;
        let my_team = self.team.unwrap_or(Team::Boys);
        for (&id, p) in &self.remote {
            if p.health == 0 || p.team == my_team {
                continue;
            }
            if let Some(d) = map::ray_box(
                origin.to_array(),
                dir.to_array(),
                [p.x - 0.38, p.y, p.z - 0.38],
                [p.x + 0.38, p.y + 1.92, p.z + 0.38],
            ) {
                if d < nearest {
                    nearest = d;
                    hit = Some(id);
                }
            }
        }
        self.spawn_shot_vfx(
            origin,
            dir,
            nearest,
            true,
            hit.is_none() && wall_dist < 95.0,
        );
        let Some(target_id) = hit else {
            return;
        };
        self.hitmark_at = self.last_frame;
        let mut killed = None;
        if let Some(target) = self.remote.get_mut(&target_id) {
            target.health = target.health.saturating_sub(1);
            target.hit_at = self.last_frame;
            let pos = Vec3::new(target.x, target.y + 1.15, target.z);
            let dead = target.health == 0;
            if dead {
                target.deaths += 1;
                target.bot_respawn_at = self.last_frame + 2600.0;
                killed = Some((target.name.clone(), target.team));
            }
            if self.show_blood {
                if self.blood.len() == 8 {
                    self.blood.remove(0);
                }
                self.blood.push(BloodEffect {
                    position: pos,
                    at: self.last_frame,
                    death: dead,
                });
            }
        }
        if let Some((victim_name, victim_team)) = killed {
            self.kills += 1;
            self.register_local_kill(&victim_name);
            if self.kill_feed.len() == 5 {
                self.kill_feed.remove(0);
            }
            self.kill_feed.push((
                self.username.clone(),
                victim_name,
                my_team,
                victim_team,
                self.last_frame,
            ));
            let idx = usize::from(my_team == Team::Girls);
            self.score[idx] += 1;
            if self.score[idx] >= self.kill_target && self.winner.is_none() {
                self.winner = Some(my_team);
                self.result_deadline = self.last_frame + 8000.0;
                web::sfx_end(self.volume, true);
                self.show_screen(Screen::Result);
            }
        } else {
            web::sfx_hit(self.volume, false, 0);
        }
    }

    fn step_practice(&mut self, dt: f32, now: f64) {
        if self.mode != Mode::Practice
            || self.screen == Screen::Paused
            || self.screen == Screen::Settings
        {
            return;
        }
        if self.winner.is_some() {
            if now >= self.result_deadline {
                let back = self.practice_back;
                self.start_practice(back);
            }
            return;
        }
        let ids: Vec<u32> = self.remote.keys().copied().collect();
        let my_team = self.team.unwrap_or(Team::Boys);
        let my_pos = Vec3::new(
            self.position.x,
            self.position.y - map::EYE_HEIGHT,
            self.position.z,
        );
        for id in ids {
            let (team, alive, respawn_at) = {
                let b = &self.remote[&id];
                (b.team, b.health > 0, b.bot_respawn_at)
            };
            if !alive {
                if now >= respawn_at {
                    let spawn = self.spawn_point(team);
                    let wp = if team == Team::Boys { 1 } else { 17 };
                    if let Some(b) = self.remote.get_mut(&id) {
                        (b.x, b.y, b.z) = (spawn.x, spawn.y, spawn.z);
                        b.target = spawn;
                        b.health = 3;
                        b.bot_wp = wp;
                        b.bot_fire_at = now + 1100.0;
                    }
                }
                continue;
            }
            // Find nearest visible enemy (either player or opposing bot).
            let (bx, by, bz, mut byaw, mut bwp, bstrafe, bfire) = {
                let b = &self.remote[&id];
                (b.x, b.y, b.z, b.yaw, b.bot_wp, b.bot_strafe, b.bot_fire_at)
            };
            let eye = Vec3::new(bx, by + 1.65, bz);
            let mut best_target: Option<(Option<u32>, Vec3, f32)> = None;
            if self.health > 0 && my_team != team {
                let aim = Vec3::new(my_pos.x, my_pos.y + 1.25, my_pos.z);
                let delta = aim - eye;
                let dist = delta.length();
                if dist > 0.5 && dist < 34.0 {
                    let dir = delta / dist;
                    if map::ray_map(eye.to_array(), dir.to_array()) > dist - 0.6 {
                        best_target = Some((None, aim, dist));
                    }
                }
            }
            for (&other_id, other) in &self.remote {
                if other_id == id || other.health == 0 || other.team == team {
                    continue;
                }
                let aim = Vec3::new(other.x, other.y + 1.25, other.z);
                let delta = aim - eye;
                let dist = delta.length();
                if dist > 0.5 && dist < 34.0 && best_target.is_none_or(|(_, _, d)| dist < d) {
                    let dir = delta / dist;
                    if map::ray_map(eye.to_array(), dir.to_array()) > dist - 0.6 {
                        best_target = Some((Some(other_id), aim, dist));
                    }
                }
            }

            let mut nx = bx;
            let mut ny = by;
            let mut nz = bz;
            let mut shot_event = None;
            if let Some((target_opt, aim, dist)) = best_target {
                let delta = aim - eye;
                byaw = delta.z.atan2(delta.x);
                let strafe_dir = Vec3::new(-delta.z, 0.0, delta.x).normalize_or_zero() * bstrafe;
                let step = strafe_dir * 2.1 * dt;
                let tx = bx + step.x;
                let tz = bz + step.z;
                if let Some(fl) = map::stand_height(Map::Kervan, tx, tz, by, 0.28) {
                    (nx, ny, nz) = (tx, fl, tz);
                }
                if now >= bfire {
                    let spread_y = (self.next_rand() - 0.5) * 0.19;
                    let spread_p = (self.next_rand() - 0.5) * 0.14;
                    let pitch = (delta.y / dist).asin() + spread_p;
                    let yaw = byaw + spread_y;
                    let dir = Vec3::new(
                        yaw.cos() * pitch.cos(),
                        pitch.sin(),
                        yaw.sin() * pitch.cos(),
                    );
                    let hit_ok = self.next_rand() < if target_opt.is_none() { 0.34 } else { 0.42 };
                    let next_cd = 950.0 + self.next_rand() as f64 * 650.0;
                    shot_event = Some((eye, dir, dist, target_opt, hit_ok, now + next_cd));
                }
            } else {
                let (wp_pos, neighbors) = map::KERVAN_WAYPOINTS[bwp % map::KERVAN_WAYPOINTS.len()];
                let dx = wp_pos[0] - bx;
                let dz = wp_pos[1] - bz;
                let d = dx.hypot(dz);
                if d < 2.2 {
                    let idx =
                        (self.next_rand() * neighbors.len() as f32) as usize % neighbors.len();
                    bwp = neighbors[idx];
                } else {
                    byaw = dz.atan2(dx);
                    let speed = 3.35 * dt;
                    let sx = dx / d * speed;
                    let sz = dz / d * speed;
                    if let Some(fl) = map::stand_height(Map::Kervan, bx + sx, bz + sz, by, 0.28) {
                        (nx, ny, nz) = (bx + sx, fl, bz + sz);
                    } else if let Some(fl) = map::stand_height(Map::Kervan, bx + sx, bz, by, 0.28) {
                        (nx, ny) = (bx + sx, fl);
                    } else if let Some(fl) = map::stand_height(Map::Kervan, bx, bz + sz, by, 0.28) {
                        (ny, nz) = (fl, bz + sz);
                    } else {
                        let idx =
                            (self.next_rand() * neighbors.len() as f32) as usize % neighbors.len();
                        bwp = neighbors[idx];
                    }
                }
            }
            if let Some(b) = self.remote.get_mut(&id) {
                (b.x, b.y, b.z) = (nx, ny, nz);
                b.target = Vec3::new(nx, ny, nz);
                b.yaw = byaw;
                b.bot_wp = bwp;
                if let Some((_, _, _, _, _, next_at)) = shot_event {
                    b.bot_fire_at = next_at;
                    b.last_shot_at = now;
                }
            }
            if let Some((origin, dir, dist, target_opt, hit_ok, _)) = shot_event {
                let wall_d = map::ray_map(origin.to_array(), dir.to_array());
                let tracer_d = if hit_ok { dist.min(wall_d) } else { wall_d };
                self.spawn_shot_vfx(origin, dir, tracer_d, false, !hit_ok && wall_d < 95.0);
                web::sfx_fire(self.volume, origin.distance(self.position));
                if hit_ok {
                    let killer_name = self.remote[&id].name.clone();
                    match target_opt {
                        None if self.health > 0 => {
                            self.health -= 1;
                            self.local_hit_at = now;
                            self.damage_angle = Some((
                                (origin.z - self.position.z).atan2(origin.x - self.position.x),
                                now,
                            ));
                            web::sfx_hurt(self.volume);
                            if self.health == 0 {
                                self.deaths += 1;
                                self.streak = 0;
                                self.killed_by = Some(killer_name.clone());
                                self.died_at = now;
                                if let Some(k) = self.remote.get_mut(&id) {
                                    k.kills += 1;
                                }
                                if self.kill_feed.len() == 5 {
                                    self.kill_feed.remove(0);
                                }
                                self.kill_feed.push((
                                    killer_name,
                                    self.username.clone(),
                                    team,
                                    my_team,
                                    now,
                                ));
                                let idx = usize::from(team == Team::Girls);
                                self.score[idx] += 1;
                                if self.score[idx] >= self.kill_target {
                                    self.winner = Some(team);
                                    self.result_deadline = now + 8000.0;
                                    web::sfx_end(self.volume, false);
                                    self.show_screen(Screen::Result);
                                } else {
                                    self.show_screen(Screen::Dead);
                                }
                            }
                        }
                        Some(vic_id) => {
                            let mut dead_info = None;
                            if let Some(vic) = self.remote.get_mut(&vic_id) {
                                if vic.health > 0 {
                                    vic.health -= 1;
                                    vic.hit_at = now;
                                    if vic.health == 0 {
                                        vic.deaths += 1;
                                        vic.bot_respawn_at = now + 2600.0;
                                        dead_info = Some((
                                            vic.name.clone(),
                                            vic.team,
                                            Vec3::new(vic.x, vic.y + 1.1, vic.z),
                                        ));
                                    }
                                }
                            }
                            if let Some((vname, vteam, pos)) = dead_info {
                                if let Some(k) = self.remote.get_mut(&id) {
                                    k.kills += 1;
                                }
                                if self.show_blood {
                                    if self.blood.len() == 8 {
                                        self.blood.remove(0);
                                    }
                                    self.blood.push(BloodEffect {
                                        position: pos,
                                        at: now,
                                        death: true,
                                    });
                                }
                                if self.kill_feed.len() == 5 {
                                    self.kill_feed.remove(0);
                                }
                                self.kill_feed.push((killer_name, vname, team, vteam, now));
                                let idx = usize::from(team == Team::Girls);
                                self.score[idx] += 1;
                                if self.score[idx] >= self.kill_target && self.winner.is_none() {
                                    self.winner = Some(team);
                                    self.result_deadline = now + 8000.0;
                                    web::sfx_end(self.volume, team == my_team);
                                    self.show_screen(Screen::Result);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
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
        if self.touched[1] && !auth_fits(&self.username, &self.password, self.register_tab) {
            return Some((Field::Password,"Kullanıcı adı ve şifre 256 baytlık ileti sınırını aşıyor. Daha kısa bir şifre kullanın.".to_owned()));
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
                self.show_screen(if self.mode == Mode::Ranked && !self.ranked_started {
                    Screen::MatchIntro
                } else if self.winner.is_some() {
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
                i.pointer.button_down(egui::PointerButton::Primary),
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
        self.weapon_sway = (self.weapon_sway + vec2(-delta.x * 0.00055, delta.y * 0.00045))
            .clamp(vec2(-0.075, -0.055), vec2(0.075, 0.055));
        if !focused {
            self.show_screen(Screen::Paused);
            return;
        }
        if fire && self.health > 0 && self.last_frame - self.shot_at >= FIRE_COOLDOWN_MS {
            self.shot_at = self.last_frame;
            self.recoil_pitch = (self.recoil_pitch + 0.048).min(0.11);
            self.fov_kick = (self.fov_kick + 1.4).min(3.5);
            web::sfx_fire(self.volume, 0.0);
            if self.mode == Mode::Practice {
                self.practice_fire_local();
            } else if self.connected {
                let dir = Vec3::new(
                    self.yaw.cos() * self.pitch.cos(),
                    self.pitch.sin(),
                    self.yaw.sin() * self.pitch.cos(),
                );
                let dist = map::ray_map(self.position.to_array(), dir.to_array());
                self.spawn_shot_vfx(self.position, dir, dist, true, dist < 95.0);
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
        let ctx = self.ctx.clone();
        self.socket = Some(
            ewebsock::connect_with_wakeup(url, Default::default(), move || ctx.request_repaint())
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
        self.tracers.clear();
        self.sparks.clear();
        self.kills = 0;
        self.deaths = 0;
        self.streak = 0;
        self.best_streak = 0;
        self.multi_kill = 0;
        self.last_kill_at = f64::NEG_INFINITY;
        self.kill_hit_at = f64::NEG_INFINITY;
        self.kill_banner = None;
        self.damage_angle = None;
        self.last_countdown_sec = 0;
        self.recoil_pitch = 0.0;
        self.strafe_roll = 0.0;
        self.fov_kick = 0.0;
        self.weapon_sway = Vec2::ZERO;
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
        if self.mode != Mode::Ranked {
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
                web::sfx_tick(self.volume, true);
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
                web::sfx_end(self.volume, fields[0] == "win");
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
                web::sfx_hit(self.volume, false, 0);
            }
            Some("SHOT") => {
                let (Some(Ok(id)), Some(Ok(yaw)), Some(Ok(pitch))) = (
                    parts.next().map(str::parse::<u32>),
                    parts.next().map(str::parse::<f32>),
                    parts.next().map(str::parse::<f32>),
                ) else {
                    return;
                };
                if let Some(player) = self.remote.get_mut(&id) {
                    player.last_shot_at = self.last_frame;
                    let origin = Vec3::new(player.x, player.y + 1.65, player.z);
                    let dir = Vec3::new(
                        yaw.cos() * pitch.cos(),
                        pitch.sin(),
                        yaw.sin() * pitch.cos(),
                    );
                    let dist = map::ray_map(origin.to_array(), dir.to_array());
                    let listener_dist = origin.distance(self.position);
                    self.spawn_shot_vfx(origin, dir, dist, false, dist < 95.0);
                    web::sfx_fire(self.volume, listener_dist);
                }
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
                    self.register_local_kill(&victim_name);
                } else if let Some(p) = self.remote.get_mut(&killer) {
                    p.kills += 1;
                }
                if self.self_id == Some(victim) {
                    self.deaths += 1;
                    self.streak = 0;
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
                        self.yaw = if team == "girls" {
                            std::f32::consts::PI
                        } else {
                            -std::f32::consts::FRAC_PI_2
                        };
                        self.pitch = 0.0;
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
                    web::sfx_end(self.volume, self.team == self.winner);
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
                        self.yaw = if self.team == Some(Team::Girls) {
                            std::f32::consts::PI
                        } else {
                            -std::f32::consts::FRAC_PI_2
                        };
                        self.pitch = 0.0;
                        // AT is a respawn/reset; its HP frame can arrive on the next UI frame.
                        self.health = 3;
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
                            hit_at: f64::NEG_INFINITY,
                            last_shot_at: f64::NEG_INFINITY,
                            walk_phase: id as f32 * 0.9,
                            move_speed: 0.0,
                            bot_wp: 0,
                            bot_fire_at: f64::NEG_INFINITY,
                            bot_strafe: 1.0,
                            bot_respawn_at: f64::NEG_INFINITY,
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
                            web::sfx_hurt(self.volume);
                            // Point damage arc toward the enemy who fired most recently.
                            if let Some(shooter) = self
                                .remote
                                .values()
                                .filter(|p| {
                                    p.health > 0
                                        && Some(p.team) != self.team
                                        && self.last_frame - p.last_shot_at < 600.0
                                })
                                .max_by(|a, b| a.last_shot_at.total_cmp(&b.last_shot_at))
                            {
                                self.damage_angle = Some((
                                    (shooter.z - self.position.z)
                                        .atan2(shooter.x - self.position.x),
                                    self.last_frame,
                                ));
                            }
                            if self.show_blood {
                                self.local_hit_at = self.last_frame;
                            }
                        }
                        self.health = health;
                        if health == 0 && self.winner.is_none() {
                            self.streak = 0;
                            self.died_at = self.last_frame;
                            self.show_screen(Screen::Dead);
                        }
                    } else if let Some(player) = self.remote.get_mut(&id) {
                        if health < player.health {
                            hit_position = Some(Vec3::new(player.x, player.y + 1.1, player.z));
                            player.hit_at = self.last_frame;
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
        let dt = ((now - self.last_draw) / 1000.0).clamp(0.0, 0.05) as f32;
        self.last_frame = now;
        let prev_pos: HashMap<u32, (f32, f32)> = self
            .remote
            .iter()
            .map(|(&id, p)| (id, (p.x, p.z)))
            .collect();
        if self.mode == Mode::Practice {
            self.step_practice(dt, now);
        } else {
            let blend = (dt * 15.0).min(1.0);
            for player in self.remote.values_mut() {
                let eased = Vec3::new(player.x, player.y, player.z).lerp(player.target, blend);
                (player.x, player.y, player.z) = (eased.x, eased.y, eased.z);
            }
        }
        if dt > 0.0001 {
            let smooth = (dt * 10.0).min(1.0);
            for (&id, player) in self.remote.iter_mut() {
                let (px, pz) = prev_pos.get(&id).copied().unwrap_or((player.x, player.z));
                let inst_speed = ((player.x - px).hypot(player.z - pz) / dt).min(6.0);
                player.move_speed += (inst_speed - player.move_speed) * smooth;
                if player.move_speed > 0.15 {
                    player.walk_phase += player.move_speed * dt * 2.55;
                }
            }
        }
        let backdrop = matches!(
            self.screen,
            Screen::Boot
                | Screen::Auth
                | Screen::Lobby
                | Screen::Queue
                | Screen::Disconnected
                | Screen::RankedResult
        ) || (self.screen == Screen::Settings
            && self.settings_back == Screen::Lobby);
        if backdrop && self.backdrop_slow >= 3 {
            return false;
        }
        if self.target == [width, height]
            && self.fps_limit > 0
            && now - self.last_draw < 1000.0 / self.fps_limit as f64
        {
            return true;
        }
        self.last_draw = now;
        self.fps_frames += 1;
        let forward = Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin());
        let right = Vec3::new(-forward.z, 0.0, forward.x);
        let strafe_input = (self.keys[3] as i32 - self.keys[1] as i32) as f32;
        let movement =
            forward * (self.keys[0] as i32 - self.keys[2] as i32) as f32 + right * strafe_input;
        let mut moved_dist = 0.0_f32;
        if self.screen == Screen::Playing && self.health > 0 {
            let radius = 0.28;
            let feet = self.position.y - map::EYE_HEIGHT;
            let before_x = self.position.x;
            let before_z = self.position.z;
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
            moved_dist = (self.position.x - before_x).hypot(self.position.z - before_z);
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
                    self.was_airborne = true;
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
                if self.was_airborne && self.vertical < -3.0 {
                    self.land_dip = (self.vertical.abs() * 0.012).min(0.09);
                    web::sfx_step(self.volume, true);
                }
                self.was_airborne = false;
                self.position.y = floor + map::EYE_HEIGHT;
                self.vertical = 0.0;
                self.ground = self.position;
            } else {
                self.was_airborne = true;
                self.position.y += self.vertical * dt;
            }
            if !self.was_airborne && moved_dist > 0.001 {
                self.step_dist += moved_dist;
                if self.step_dist >= 1.85 {
                    self.step_dist = 0.0;
                    web::sfx_step(self.volume, false);
                }
            }
            // Back to the last floor, which the server also knows, not a fixed spawn.
            if self.position.y < FALL_LIMIT {
                self.position = self.ground;
                self.vertical = 0.0;
            }
        }
        // Smooth camera and weapon feel damping.
        let damp = (-dt * 12.0).exp();
        self.recoil_pitch *= damp;
        self.fov_kick *= (-dt * 10.0).exp();
        self.weapon_sway *= (-dt * 11.0).exp();
        self.land_dip *= (-dt * 13.0).exp();
        let target_roll = if self.screen == Screen::Playing && self.health > 0 {
            -strafe_input * 0.022
        } else {
            0.0
        };
        self.strafe_roll += (target_roll - self.strafe_roll) * (dt * 10.0).min(1.0);
        let moving_ground = !self.was_airborne && moved_dist > 0.002;
        let target_bob = if moving_ground { 1.0 } else { 0.0 };
        self.bob_amount += (target_bob - self.bob_amount) * (dt * 10.0).min(1.0);
        if moving_ground {
            self.bob_phase += dt * 10.5;
        }

        if self.screen == Screen::Playing
            && self.mode != Mode::Practice
            && self.connected
            && now - self.last_sent >= 31.25
        {
            self.send(format!(
                "POS|{:.3}|{:.3}|{:.3}|{:.3}",
                self.position.x,
                self.position.z,
                self.position.y - map::EYE_HEIGHT,
                self.yaw
            ));
            self.last_sent = now;
        }

        let aspect = width as f32 / height as f32;
        let fov_rad = (70.0 + self.fov_kick).to_radians();
        let projection = camera::rh::proj::opengl::perspective(fov_rad, aspect, 0.1, 145.0);
        let (cam_pos, cam_fwd, cam_up) = if backdrop {
            let angle = now as f32 * 0.000085;
            let center = Vec3::new(0.0, 5.2, -4.0);
            let eye = center
                + Vec3::new(
                    angle.cos() * 24.0,
                    12.5 + (angle * 1.7).sin() * 2.5,
                    angle.sin() * 26.0,
                );
            (eye, (center - eye).normalize(), Vec3::Y)
        } else {
            let hurt_shake = if now - self.local_hit_at < 180.0 {
                let t = (1.0 - (now - self.local_hit_at) as f32 / 180.0).clamp(0.0, 1.0);
                (
                    (now as f32 * 0.07).sin() * 0.018 * t,
                    (now as f32 * 0.09).cos() * 0.014 * t,
                )
            } else {
                (0.0, 0.0)
            };
            let vp_pitch = (self.pitch + self.recoil_pitch + hurt_shake.1).clamp(-1.54, 1.54);
            let fwd = Vec3::new(
                self.yaw.cos() * vp_pitch.cos(),
                vp_pitch.sin(),
                self.yaw.sin() * vp_pitch.cos(),
            )
            .normalize();
            let base_right = fwd.cross(Vec3::Y).normalize_or_zero();
            let base_up = base_right.cross(fwd).normalize_or_zero();
            let roll = self.strafe_roll + hurt_shake.0;
            let rolled_up = (base_up * roll.cos() + base_right * roll.sin()).normalize();
            let eye = self.position - Vec3::Y * (self.land_dip * 0.85);
            (eye, fwd, rolled_up)
        };
        let cam_right = cam_fwd.cross(cam_up).normalize_or_zero();
        let cam_up_ortho = cam_right.cross(cam_fwd).normalize_or_zero();
        let view = camera::rh::view::look_to_mat4(cam_pos, cam_fwd, cam_up_ortho);
        let vp = projection * view;
        self.last_vp = vp;

        let muzzle_age = ((now - self.shot_at) / 110.0) as f32;
        let muzzle_light = if !backdrop && (0.0..1.0).contains(&muzzle_age) {
            (1.0 - muzzle_age) * 1.35
        } else {
            0.0
        };
        let muzzle_pos = cam_pos + cam_fwd * 1.1;

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
            gl.clear_color(0.89, 0.79, 0.65, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);

            // Golden desert sky gradient & sun disc.
            gl.disable(glow::DEPTH_TEST);
            gl.depth_mask(false);
            gl.use_program(Some(self.sky_program));
            gl.uniform_3_f32(Some(&self.sky_fwd_loc), cam_fwd.x, cam_fwd.y, cam_fwd.z);
            gl.uniform_3_f32(
                Some(&self.sky_right_loc),
                cam_right.x,
                cam_right.y,
                cam_right.z,
            );
            gl.uniform_3_f32(
                Some(&self.sky_up_loc),
                cam_up_ortho.x,
                cam_up_ortho.y,
                cam_up_ortho.z,
            );
            gl.uniform_1_f32(Some(&self.sky_aspect_loc), aspect);
            gl.uniform_1_f32(Some(&self.sky_fov_loc), (fov_rad * 0.5).tan());
            gl.bind_vertex_array(Some(self.sprite_vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 6);
            gl.depth_mask(true);
            gl.enable(glow::DEPTH_TEST);

            gl.use_program(Some(self.program));
            gl.active_texture(glow::TEXTURE0);
            gl.uniform_3_f32(Some(&self.cam_pos_loc), cam_pos.x, cam_pos.y, cam_pos.z);
            gl.uniform_3_f32(
                Some(&self.muzzle_pos_loc),
                muzzle_pos.x,
                muzzle_pos.y,
                muzzle_pos.z,
            );
            gl.uniform_1_f32(Some(&self.muzzle_light_loc), muzzle_light);
            gl.uniform_1_i32(Some(&self.texture_enabled), 0);
            gl.uniform_1_i32(Some(&self.use_vnormal_loc), 0);
            gl.uniform_1_i32(Some(&self.unlit_loc), 0);
            gl.uniform_1_i32(Some(&self.no_fog_loc), 0);
            gl.uniform_4_f32(Some(&self.hit_flash_loc), 0.0, 0.0, 0.0, 0.0);
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
                gl.uniform_matrix_4_f32_slice(Some(&self.model_loc), false, &model.to_cols_array());
                gl.vertex_attrib_4_f32(3, block.color[0], block.color[1], block.color[2], 1.0);
                gl.draw_elements(glow::TRIANGLES, 36, glow::UNSIGNED_SHORT, 0);
            }
            if self.map == Map::Kervan {
                gl.bind_vertex_array(Some(self.kervan_vao));
                let kervan_model = Mat4::from_scale(Vec3::splat(map::KERVAN_SCALE));
                let kervan_vp = vp * kervan_model;
                gl.uniform_matrix_4_f32_slice(
                    Some(&self.transform),
                    false,
                    &kervan_vp.to_cols_array(),
                );
                gl.uniform_matrix_4_f32_slice(
                    Some(&self.model_loc),
                    false,
                    &kervan_model.to_cols_array(),
                );
                gl.uniform_1_i32(Some(&self.texture_enabled), 1);
                gl.vertex_attrib_4_f32(3, 1.0, 1.0, 1.0, 1.0);
                for part in &self.kervan_parts {
                    gl.bind_texture(glow::TEXTURE_2D, Some(self.kervan_textures[part.image]));
                    gl.draw_arrays(glow::TRIANGLES, part.first, part.count);
                }
                gl.bind_vertex_array(Some(self.vao));
            }
            gl.uniform_1_i32(Some(&self.texture_enabled), 0);
            gl.uniform_1_i32(Some(&self.unlit_loc), 1);
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
                        gl.uniform_matrix_4_f32_slice(
                            Some(&self.model_loc),
                            false,
                            &model.to_cols_array(),
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

            // Wall impact dust & sparks + glowing 3D bullet tracers.
            self.sparks.retain(|s| now - s.at < 320.0);
            self.tracers.retain(|t| now - t.at < 140.0);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.depth_mask(false);
            for spark in &self.sparks {
                let age = ((now - spark.at) / 1000.0).max(0.0) as f32;
                let alpha = (1.0 - age / 0.32).clamp(0.0, 1.0);
                let pos = spark.position + spark.velocity * age - Vec3::Y * (4.5 * age * age);
                let model = Mat4::from_translation(pos)
                    * Mat4::from_scale(Vec3::splat(0.045 * alpha.max(0.3)));
                gl.uniform_matrix_4_f32_slice(
                    Some(&self.transform),
                    false,
                    &(vp * model).to_cols_array(),
                );
                gl.uniform_matrix_4_f32_slice(Some(&self.model_loc), false, &model.to_cols_array());
                gl.vertex_attrib_4_f32(3, 1.0, 0.78, 0.38, alpha * 0.9);
                gl.draw_elements(glow::TRIANGLES, 36, glow::UNSIGNED_SHORT, 0);
            }
            for tracer in &self.tracers {
                let age = ((now - tracer.at) / 140.0).clamp(0.0, 1.0) as f32;
                let delta = tracer.end - tracer.start;
                let len = delta.length();
                if len < 0.2 {
                    continue;
                }
                let dir = delta / len;
                let up_ref = if dir.y.abs() > 0.95 { Vec3::X } else { Vec3::Y };
                let tr = dir.cross(up_ref).normalize();
                let tu = tr.cross(dir).normalize();
                let thick = if tracer.mine { 0.016 } else { 0.022 } * (1.0 - age * 0.5);
                let mid = (tracer.start + tracer.end) * 0.5;
                let model = Mat4::from_cols(
                    (tr * thick).extend(0.0),
                    (tu * thick).extend(0.0),
                    (dir * (len * 0.5)).extend(0.0),
                    mid.extend(1.0),
                );
                gl.uniform_matrix_4_f32_slice(
                    Some(&self.transform),
                    false,
                    &(vp * model).to_cols_array(),
                );
                gl.uniform_matrix_4_f32_slice(Some(&self.model_loc), false, &model.to_cols_array());
                gl.vertex_attrib_4_f32(3, 1.0, 0.82, 0.32, (1.0 - age) * 0.85);
                gl.draw_elements(glow::TRIANGLES, 36, glow::UNSIGNED_SHORT, 0);
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
            // Soft contact shadows under 3D characters.
            for player in &players {
                let model = Mat4::from_translation(Vec3::new(player.x, player.y + 0.03, player.z))
                    * Mat4::from_scale(Vec3::new(0.42, 0.01, 0.42));
                gl.uniform_matrix_4_f32_slice(
                    Some(&self.transform),
                    false,
                    &(vp * model).to_cols_array(),
                );
                gl.uniform_matrix_4_f32_slice(Some(&self.model_loc), false, &model.to_cols_array());
                gl.vertex_attrib_4_f32(3, 0.05, 0.04, 0.03, 0.42);
                gl.draw_elements(glow::TRIANGLES, 36, glow::UNSIGNED_SHORT, 0);
            }
            gl.disable(glow::BLEND);
            gl.depth_mask(true);

            // Articulated 3D animated characters (Boys = Male Operative, Girls = Female Operative).
            gl.uniform_1_i32(Some(&self.texture_enabled), 1);
            gl.uniform_1_i32(Some(&self.use_vnormal_loc), 1);
            gl.uniform_1_i32(Some(&self.unlit_loc), 0);
            gl.uniform_1_i32(Some(&self.no_fog_loc), 0);
            let mut anim_offsets: Vec<(usize, Vec3, Quat, Vec3)> = Vec::with_capacity(12);
            for player in &players {
                let model = match player.team {
                    Team::Boys => &self.male_model,
                    Team::Girls => &self.female_model,
                };
                let run = (player.move_speed / 3.4).clamp(0.0, 1.0);
                let s = player.walk_phase.sin();
                let c = player.walk_phase.cos();
                let breath = ((now * 0.0024) as f32 + player.x).sin() * 0.018 * (1.0 - run);
                let hit_fade = ((140.0 - (now - player.hit_at)) / 140.0).clamp(0.0, 1.0) as f32;
                let shot_kick =
                    ((120.0 - (now - player.last_shot_at)) / 120.0).clamp(0.0, 1.0) as f32;

                anim_offsets.clear();
                let mut push_anim = |name: &str, t: Vec3, r: Quat| {
                    if let Some(idx) = model.node_index(name) {
                        anim_offsets.push((idx, t, r, Vec3::ONE));
                    }
                };
                push_anim(
                    "pelvis",
                    Vec3::new(0.0, s.abs() * 0.042 * run, 0.0),
                    Quat::from_rotation_y(s * 0.12 * run),
                );
                push_anim(
                    "torso",
                    Vec3::ZERO,
                    Quat::from_rotation_y(-s * 0.14 * run)
                        * Quat::from_rotation_x(
                            0.05 * run + breath - shot_kick * 0.09 - hit_fade * 0.16,
                        ),
                );
                push_anim(
                    "head",
                    Vec3::ZERO,
                    Quat::from_rotation_y(s * 0.08 * run)
                        * Quat::from_rotation_x(-breath * 0.5 + hit_fade * 0.12),
                );
                push_anim(
                    "ponytail",
                    Vec3::ZERO,
                    Quat::from_rotation_x((0.14 + s.abs() * 0.28) * run)
                        * Quat::from_rotation_y(-s * 0.24 * run),
                );
                push_anim(
                    "rifle",
                    Vec3::new(0.0, 0.0, -shot_kick * 0.045),
                    Quat::from_rotation_x(-shot_kick * 0.09)
                        * Quat::from_rotation_z(s * 0.03 * run),
                );
                push_anim(
                    "upper_arm_l",
                    Vec3::ZERO,
                    Quat::from_rotation_x(-shot_kick * 0.06),
                );
                push_anim(
                    "upper_arm_r",
                    Vec3::ZERO,
                    Quat::from_rotation_x(-shot_kick * 0.08),
                );
                push_anim(
                    "thigh_l",
                    Vec3::ZERO,
                    Quat::from_rotation_x(-s * 0.58 * run),
                );
                push_anim(
                    "shin_l",
                    Vec3::ZERO,
                    Quat::from_rotation_x((0.06 + c.max(0.0) * 0.78 + (-s).max(0.0) * 0.20) * run),
                );
                push_anim(
                    "foot_l",
                    Vec3::ZERO,
                    Quat::from_rotation_x((-s * 0.20 - c.max(0.0) * 0.26) * run),
                );
                push_anim("thigh_r", Vec3::ZERO, Quat::from_rotation_x(s * 0.58 * run));
                push_anim(
                    "shin_r",
                    Vec3::ZERO,
                    Quat::from_rotation_x((0.06 + (-c).max(0.0) * 0.78 + s.max(0.0) * 0.20) * run),
                );
                push_anim(
                    "foot_r",
                    Vec3::ZERO,
                    Quat::from_rotation_x((s * 0.20 - (-c).max(0.0) * 0.26) * run),
                );

                let root = Mat4::from_translation(Vec3::new(player.x, player.y, player.z))
                    * Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2 - player.yaw);
                let mats = model.compute_world(root, &anim_offsets);
                let flash_a = if hit_fade > 0.01 {
                    hit_fade * 0.65
                } else {
                    0.0
                };
                gl.uniform_4_f32(Some(&self.hit_flash_loc), 1.0, 0.35, 0.25, flash_a);
                gl.vertex_attrib_4_f32(3, 1.0 + shot_kick * 0.22, 1.0 + shot_kick * 0.12, 1.0, 1.0);
                gl.bind_vertex_array(Some(model.vao));
                for part in &model.parts {
                    let m = mats[part.node_idx];
                    gl.uniform_matrix_4_f32_slice(
                        Some(&self.transform),
                        false,
                        &(vp * m).to_cols_array(),
                    );
                    gl.uniform_matrix_4_f32_slice(Some(&self.model_loc), false, &m.to_cols_array());
                    gl.bind_texture(glow::TEXTURE_2D, Some(model.textures[part.image]));
                    gl.draw_arrays(glow::TRIANGLES, part.first, part.count);
                }

                // Bright 3D muzzle spark when remote player/bot fires.
                if now - player.last_shot_at < 75.0 {
                    let fwd_p = Vec3::new(player.yaw.cos(), 0.0, player.yaw.sin());
                    let muzzle = Vec3::new(player.x, player.y + 1.37, player.z) + fwd_p * 0.82;
                    let m = Mat4::from_translation(muzzle)
                        * Mat4::from_rotation_y(player.yaw + now as f32 * 0.05)
                        * Mat4::from_scale(Vec3::splat(0.11));
                    gl.bind_vertex_array(Some(self.vao));
                    gl.uniform_1_i32(Some(&self.texture_enabled), 0);
                    gl.uniform_1_i32(Some(&self.use_vnormal_loc), 0);
                    gl.uniform_1_i32(Some(&self.unlit_loc), 1);
                    gl.uniform_4_f32(Some(&self.hit_flash_loc), 0.0, 0.0, 0.0, 0.0);
                    gl.uniform_matrix_4_f32_slice(
                        Some(&self.transform),
                        false,
                        &(vp * m).to_cols_array(),
                    );
                    gl.uniform_matrix_4_f32_slice(Some(&self.model_loc), false, &m.to_cols_array());
                    gl.vertex_attrib_4_f32(3, 1.0, 0.82, 0.32, 1.0);
                    gl.draw_elements(glow::TRIANGLES, 36, glow::UNSIGNED_SHORT, 0);
                    gl.uniform_1_i32(Some(&self.texture_enabled), 1);
                    gl.uniform_1_i32(Some(&self.use_vnormal_loc), 1);
                    gl.uniform_1_i32(Some(&self.unlit_loc), 0);
                }
            }

            // Hero 3D First-Person AK-47 Viewmodel (camera-space depth pass so it never clips walls).
            if !backdrop {
                gl.clear(glow::DEPTH_BUFFER_BIT);
                gl.enable(glow::DEPTH_TEST);
                gl.depth_mask(true);
                let since_shot = now - self.shot_at;
                let kick = if (0.0..240.0).contains(&since_shot) {
                    (1.0 - (since_shot as f32 / 240.0)).powi(2)
                } else {
                    0.0
                };
                let bolt_t = if (0.0..130.0).contains(&since_shot) {
                    ((since_shot as f32 / 130.0) * std::f32::consts::PI).sin()
                } else {
                    0.0
                };
                let show_flash = (0.0..80.0).contains(&since_shot);
                let bob_x = self.bob_phase.cos() * 0.013 * self.bob_amount;
                let bob_y = -(self.bob_phase * 2.0).sin().abs() * 0.011 * self.bob_amount;
                let idle_b = (now * 0.0022) as f32;
                let idle_y = idle_b.sin() * 0.0022 * (1.0 - self.bob_amount);
                let w_tx = bob_x + self.weapon_sway.x * 0.16;
                let w_ty = bob_y + idle_y + self.weapon_sway.y * 0.12 - self.land_dip * 0.25;
                let w_tz = 0.035 + kick * 0.055;
                let w_rot = Quat::from_rotation_y(-self.weapon_sway.x * 0.48)
                    * Quat::from_rotation_x(self.weapon_sway.y * 0.36 + kick * 0.09)
                    * Quat::from_rotation_z(
                        -self.strafe_roll * 1.35 - self.weapon_sway.x * 0.40 + kick * 0.032,
                    );
                let weapon_root =
                    Mat4::from_translation(Vec3::new(w_tx, w_ty, w_tz)) * Mat4::from_quat(w_rot);
                let weapon_proj = camera::rh::proj::opengl::perspective(
                    (58.0 + self.fov_kick * 0.5).to_radians(),
                    aspect,
                    0.02,
                    8.0,
                );
                let cam_to_world = view.inverse();
                let mut w_offsets = Vec::with_capacity(2);
                if let Some(bc) = self.weapon_model.node_index("bolt_carrier") {
                    w_offsets.push((
                        bc,
                        Vec3::new(0.0, 0.0, bolt_t * 0.085),
                        Quat::IDENTITY,
                        Vec3::ONE,
                    ));
                }
                let flash_idx = self.weapon_model.node_index("muzzle_flash");
                if let (Some(mf), true) = (flash_idx, show_flash) {
                    let f_scale = 0.85 + (1.0 - (since_shot as f32 / 80.0)) * 0.35;
                    w_offsets.push((
                        mf,
                        Vec3::ZERO,
                        Quat::from_rotation_z((self.shot_at * 0.037) as f32),
                        Vec3::splat(f_scale),
                    ));
                }
                let mats = self.weapon_model.compute_world(weapon_root, &w_offsets);
                gl.bind_vertex_array(Some(self.weapon_model.vao));
                gl.uniform_1_i32(Some(&self.texture_enabled), 1);
                gl.uniform_1_i32(Some(&self.use_vnormal_loc), 1);
                gl.uniform_1_i32(Some(&self.no_fog_loc), 1);
                gl.uniform_4_f32(Some(&self.hit_flash_loc), 0.0, 0.0, 0.0, 0.0);
                gl.vertex_attrib_4_f32(
                    3,
                    1.02 + muzzle_light * 0.14,
                    1.01 + muzzle_light * 0.06,
                    1.0,
                    1.0,
                );
                for part in &self.weapon_model.parts {
                    let is_flash = flash_idx == Some(part.node_idx);
                    if is_flash && !show_flash {
                        continue;
                    }
                    let cam_m = mats[part.node_idx];
                    let world_m = cam_to_world * cam_m;
                    gl.uniform_1_i32(Some(&self.unlit_loc), i32::from(is_flash));
                    gl.uniform_matrix_4_f32_slice(
                        Some(&self.transform),
                        false,
                        &(weapon_proj * cam_m).to_cols_array(),
                    );
                    gl.uniform_matrix_4_f32_slice(
                        Some(&self.model_loc),
                        false,
                        &world_m.to_cols_array(),
                    );
                    gl.bind_texture(
                        glow::TEXTURE_2D,
                        Some(self.weapon_model.textures[part.image]),
                    );
                    gl.draw_arrays(glow::TRIANGLES, part.first, part.count);
                }
                gl.uniform_1_i32(Some(&self.unlit_loc), 0);
                gl.uniform_1_i32(Some(&self.no_fog_loc), 0);
                gl.uniform_1_i32(Some(&self.use_vnormal_loc), 0);
            }
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
        true
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

    fn settings_values(&self) -> [String; 10] {
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
            self.volume.to_string(),
            self.dynamic_crosshair.to_string(),
            self.show_radar.to_string(),
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
            "volume",
            "dynamic_crosshair",
            "show_radar",
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
        if let Some(v) = number("volume") {
            self.volume = v.clamp(0.0, 100.0);
        }
        if let Some(v) = web::storage_get("keles.show_fps").and_then(|v| v.parse().ok()) {
            self.show_fps = v;
        }
        if let Some(v) = web::storage_get("keles.show_blood").and_then(|v| v.parse().ok()) {
            self.show_blood = v;
        }
        if let Some(v) = web::storage_get("keles.dynamic_crosshair").and_then(|v| v.parse().ok()) {
            self.dynamic_crosshair = v;
        }
        if let Some(v) = web::storage_get("keles.show_radar").and_then(|v| v.parse().ok()) {
            self.show_radar = v;
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
}

impl eframe::App for Game {
    #[cfg(target_arch = "wasm32")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time) * 1000.0;
        self.last_frame = now;
        if now - self.fps_since >= 500.0 {
            self.fps = (self.fps_frames as f64 * 1000.0 / (now - self.fps_since)).round() as u32;
            if matches!(
                self.screen,
                Screen::Boot | Screen::Auth | Screen::Lobby | Screen::Queue | Screen::Disconnected
            ) && self.backdrop_slow < 3
                && self.fps_frames > 0
            {
                self.backdrop_slow = if self.fps < 20 {
                    self.backdrop_slow + 1
                } else {
                    0
                };
            }
            self.fps_since = now;
            self.fps_frames = 0;
        }
        // Start after WebRunner initialization, which can outlast the server's auth deadline.
        if self.screen == Screen::Boot
            && self.socket.is_none()
            && self.pending.is_none()
            && self.target != [0, 0]
        {
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
        if self.screen == Screen::MatchIntro && !self.ranked_started {
            let sec = ((self.go_at - now) / 1000.0).ceil().max(0.0) as u32;
            if sec > 0 && sec != self.last_countdown_sec {
                self.last_countdown_sec = sec;
                web::sfx_tick(self.volume, false);
            }
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
            && self.pending.is_none_or(|(action, _)| action == "RESPAWN")
            && now - self.last_respawn >= 250.0
        {
            if self.pending.is_none() {
                self.request("RESPAWN");
            } else {
                self.send("RESPAWN".to_owned());
            }
            self.last_respawn = now;
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
                let tint = gl.get_uniform_location(program, "tint");
                let flash = gl.get_uniform_location(program, "flash");
                gl.uniform_matrix_4_f32_slice(transform.as_ref(), false, &flip);
                gl.uniform_4_f32(tint.as_ref(), 1.0, 1.0, 1.0, 1.0);
                gl.uniform_4_f32(flash.as_ref(), 0.0, 0.0, 0.0, 0.0);
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
        self.toast_stack(&ctx);
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
        ui::BG.to_normalized_gamma_f32()
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
