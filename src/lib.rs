use std::{cell::RefCell, collections::HashMap, rc::Rc};

use glam::{camera, Mat4, Vec3};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{
    Event, HtmlButtonElement, HtmlCanvasElement, HtmlElement, HtmlImageElement, HtmlInputElement,
    KeyboardEvent, MessageEvent, MouseEvent, WebGl2RenderingContext as Gl, WebGlProgram,
    WebGlShader, WebGlTexture, WebGlUniformLocation, WebGlVertexArrayObject, WebSocket,
};
mod map;
use map::Map;

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

const VERTICES: [f32; 24] = [
    -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 1.0, 1.0, 1.0, -1.0, 1.0, 1.0, -1.0, -1.0, -1.0, 1.0, -1.0,
    -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, -1.0,
];
const SPRITE_VERTICES: [f32; 24] = [
    -1.0, -1.0, 0.0, 1.0, 1.0, -1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0, 1.0, 1.0,
    1.0, 0.0, -1.0, 1.0, 0.0, 0.0,
];
// Exact transparent cutouts in the supplied 1536x1024 four-view image:
// front, back, left-facing, right-facing. Coordinates are [left, top, right, bottom).
const GRAVITY: f32 = 20.0;
const JUMP_SPEED: f32 = 6.0;
const FIRE_COOLDOWN_MS: f64 = 500.0;
const FALL_LIMIT: f32 = -20.0;

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
    Main,
    Modes,
    MapSelect,
    Join,
    Team,
    Playing,
    Paused,
    Dead,
    Result,
    SettingsMain,
    SettingsPaused,
}

#[derive(Clone, Copy, PartialEq)]
enum Team {
    Boys,
    Girls,
}

impl Team {
    fn as_str(self) -> &'static str {
        match self {
            Team::Boys => "boys",
            Team::Girls => "girls",
        }
    }

    fn color(self) -> &'static str {
        match self {
            Team::Boys => "#4a90ff",
            Team::Girls => "#ff66cc",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Team::Boys => "Boys",
            Team::Girls => "Girls",
        }
    }
}

struct RemotePlayer {
    name: String,
    x: f32,
    z: f32,
    y: f32,
    yaw: f32,
    health: u8,
    team: Team,
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
    gl: Gl,
    canvas: HtmlCanvasElement,
    program: WebGlProgram,
    vao: WebGlVertexArrayObject,
    camel_vao: WebGlVertexArrayObject,
    camel_parts: Vec<CamelPart>,
    camel_textures: Vec<Option<WebGlTexture>>,
    transform: WebGlUniformLocation,
    texture_enabled: WebGlUniformLocation,
    sprite_program: WebGlProgram,
    sprite_vao: WebGlVertexArrayObject,
    operative_vaos: Vec<WebGlVertexArrayObject>,
    girl_vaos: Vec<WebGlVertexArrayObject>,
    sprite_transform: WebGlUniformLocation,
    textures: [Option<WebGlTexture>; 3],
    operative_texture: Option<WebGlTexture>,
    girl_texture: Option<WebGlTexture>,
    shot_at: f64,
    position: Vec3,
    yaw: f32,
    pitch: f32,
    keys: [bool; 4],
    jump: bool,
    vertical: f32,
    last_frame: f64,
    screen: Screen,
    map: Map,
    menu: HtmlElement,
    menu_title: HtmlElement,
    menu_buttons: Vec<HtmlButtonElement>,
    server_input: HtmlInputElement,
    nickname_input: HtmlInputElement,
    status: HtmlElement,
    settings_panel: HtmlElement,
    sensitivity_input: HtmlInputElement,
    raw_input: HtmlInputElement,
    hud_scale_input: HtmlInputElement,
    fps_input: HtmlInputElement,
    blood_input: HtmlInputElement,
    hud: HtmlElement,
    fps_hud: HtmlElement,
    fps_since: f64,
    fps_frames: u32,
    score_hud: HtmlElement,
    boys_score: HtmlElement,
    girls_score: HtmlElement,
    health_hud: HtmlElement,
    health_label: HtmlElement,
    health_fill: HtmlElement,
    health: u8,
    self_id: Option<u32>,
    socket: Option<WebSocket>,
    connected: bool,
    joined: bool,
    team: Option<Team>,
    winner: Option<Team>,
    result_deadline: f64,
    session: u32,
    last_sent: f64,
    remote: HashMap<u32, RemotePlayer>,
    blood: Vec<BloodEffect>,
    local_hit_at: f64,
}

impl Game {
    fn refresh_hud_scale(&self) {
        let scale = self.hud_scale_input.value_as_number() / 100.0;
        for (element, origin) in [
            (&self.hud, "top left"),
            (&self.fps_hud, "top right"),
            (&self.health_hud, "bottom left"),
        ] {
            let _ = element
                .style()
                .set_property("transform", &format!("scale({scale})"));
            let _ = element.style().set_property("transform-origin", origin);
        }
    }

    fn refresh_fps_visibility(&self) {
        let _ = self.fps_hud.style().set_property(
            "display",
            if self.fps_input.checked()
                && matches!(
                    self.screen,
                    Screen::Playing | Screen::Paused | Screen::SettingsPaused
                )
            {
                "block"
            } else {
                "none"
            },
        );
    }

    fn pointer_lock_failed(&mut self) {
        if self.screen == Screen::Playing && self.raw_input.checked() {
            self.status.set_inner_text(
                "Raw pointer lock could not be enabled here. Turn off Raw input in Settings to continue."
            );
            self.show_screen(Screen::Paused);
        }
    }

    fn disconnect(&mut self) {
        self.session = self.session.wrapping_add(1);
        if let Some(socket) = self.socket.take() {
            let _ = socket.close();
        }
        self.remote.clear();
        self.blood.clear();
        self.local_hit_at = f64::NEG_INFINITY;
        self.connected = false;
        self.joined = false;
        self.team = None;
        self.winner = None;
        self.self_id = None;
        self.hud.set_inner_text("");
        self.refresh_score(0, 0);
    }

    // Paints the top scoreboard; the local team's name gets a "(you)" suffix.
    fn refresh_score(&self, boys: u32, girls: u32) {
        for (element, team, score) in [
            (&self.boys_score, Team::Boys, boys),
            (&self.girls_score, Team::Girls, girls),
        ] {
            let you = if self.team == Some(team) {
                " (you)"
            } else {
                ""
            };
            element.set_inner_text(&format!("{}{}: {score}", team.label(), you));
            let _ = element.style().set_property("color", team.color());
        }
    }

    fn refresh_health(&self) {
        self.health_label
            .set_inner_text(&format!("Health {}/3", self.health));
        let _ = self
            .health_fill
            .style()
            .set_property("width", &format!("{}%", self.health as u32 * 100 / 3));
    }

    fn refresh_hud(&self) {
        if !self.connected {
            self.hud.set_inner_text("Connecting...");
            return;
        }
        if self.health == 0 {
            self.hud.set_inner_text("You are dead\nPress Respawn");
            return;
        }
        let mut names = self
            .remote
            .values()
            .map(|player| player.name.as_str())
            .collect::<Vec<_>>();
        names.sort_unstable();
        self.hud.set_inner_text(&format!(
            "You: {}\nOthers: {}",
            self.nickname_input.value(),
            if names.is_empty() {
                "none".to_owned()
            } else {
                names.join(", ")
            }
        ));
    }

    fn show_screen(&mut self, screen: Screen) {
        self.screen = screen;
        self.keys = [false; 4];
        self.jump = false;
        if matches!(screen, Screen::Playing | Screen::Main) {
            self.status.set_inner_text("");
        }
        let _ = self.menu.style().set_property(
            "display",
            if screen == Screen::Playing {
                "none"
            } else {
                "flex"
            },
        );
        let (title, labels) = match screen {
            Screen::Main => ("Keles", ["Play", "Settings", "Coming soon"]),
            Screen::Modes => ("Choose mode", ["Multiplayer", "Single player", ""]),
            Screen::MapSelect => ("Choose map", ["Camel", "Test", "Back"]),
            Screen::Join => ("Multiplayer", ["Connect", "Back", ""]),
            Screen::Team => ("Choose your team", ["Boys", "Girls", ""]),
            Screen::Paused => ("Paused", ["Resume", "Settings", "Main menu"]),
            Screen::Dead => ("You died", ["Respawn", "", ""]),
            Screen::Result => (
                match (self.team, self.winner) {
                    (Some(team), Some(winner)) if team == winner => "Victory",
                    _ => "Defeat",
                },
                ["", "", ""],
            ),
            Screen::SettingsMain | Screen::SettingsPaused => ("Settings", ["Back", "", ""]),
            Screen::Playing => ("", ["", "", ""]),
        };
        self.menu_title.set_inner_text(title);
        if matches!(screen, Screen::Modes | Screen::MapSelect) {
            self.status.set_inner_text("Press Escape to go back");
        }
        if screen == Screen::Team {
            self.status
                .set_inner_text("Pick a side. Boys and girls see different models.");
        }
        if screen == Screen::Dead {
            self.status.set_inner_text("Waiting to respawn.");
        }
        if screen == Screen::Result {
            self.status.set_inner_text("Restarting in 10");
        }
        for input in [&self.server_input, &self.nickname_input] {
            let _ = input.style().set_property(
                "display",
                if screen == Screen::Join {
                    "block"
                } else {
                    "none"
                },
            );
        }
        let _ = self.status.style().set_property(
            "display",
            if matches!(
                screen,
                Screen::Modes
                    | Screen::MapSelect
                    | Screen::Join
                    | Screen::Team
                    | Screen::Paused
                    | Screen::Dead
                    | Screen::Result
                    | Screen::SettingsMain
                    | Screen::SettingsPaused
            ) {
                "block"
            } else {
                "none"
            },
        );
        let _ = self.settings_panel.style().set_property(
            "display",
            if matches!(screen, Screen::SettingsMain | Screen::SettingsPaused) {
                "flex"
            } else {
                "none"
            },
        );
        let in_match = matches!(
            screen,
            Screen::Playing | Screen::Paused | Screen::SettingsPaused
        );
        let _ = self.hud.style().set_property(
            "display",
            if self.socket.is_some() && (in_match || screen == Screen::Dead) {
                "block"
            } else {
                "none"
            },
        );
        let _ = self.health_hud.style().set_property(
            "display",
            if in_match || screen == Screen::Dead {
                "block"
            } else {
                "none"
            },
        );
        let _ = self.score_hud.style().set_property(
            "display",
            if self.socket.is_some()
                && (in_match || matches!(screen, Screen::Dead | Screen::Result))
            {
                "block"
            } else {
                "none"
            },
        );
        self.refresh_fps_visibility();
        for (index, button) in self.menu_buttons.iter().enumerate() {
            button.set_inner_text(labels[index]);
            button.set_disabled(screen == Screen::Main && index == 2);
            let _ = button.style().set_property(
                "opacity",
                if screen == Screen::Main && index == 2 {
                    ".45"
                } else {
                    "1"
                },
            );
            let _ = button.style().set_property(
                "display",
                if labels[index].is_empty() {
                    "none"
                } else {
                    "block"
                },
            );
        }
    }

    fn draw(&mut self, now: f64) {
        if self.fps_since == 0.0 {
            self.fps_since = now;
        }
        self.fps_frames += 1;
        if now - self.fps_since >= 500.0 {
            self.fps_hud.set_inner_text(&format!(
                "FPS: {}",
                (self.fps_frames as f64 * 1000.0 / (now - self.fps_since)).round() as u32
            ));
            self.fps_since = now;
            self.fps_frames = 0;
        }
        let window = web_sys::window().unwrap();
        let width = window.inner_width().unwrap().as_f64().unwrap() as u32;
        let height = window.inner_height().unwrap().as_f64().unwrap() as u32;
        if self.canvas.width() != width || self.canvas.height() != height {
            self.canvas.set_width(width);
            self.canvas.set_height(height);
        }
        self.gl.viewport(0, 0, width as i32, height as i32);

        let dt = ((now - self.last_frame) / 1000.0).clamp(0.0, 0.05) as f32;
        self.last_frame = now;
        if self.screen == Screen::Result {
            let remaining = ((self.result_deadline - now) / 1000.0).ceil().max(0.0) as u32;
            self.status
                .set_inner_text(&format!("Restarting in {remaining}"));
        }
        if matches!(
            self.screen,
            Screen::Main | Screen::Modes | Screen::MapSelect | Screen::Join | Screen::SettingsMain
        ) {
            self.gl.clear_color(0.08, 0.11, 0.16, 1.0);
            self.gl.clear(Gl::COLOR_BUFFER_BIT | Gl::DEPTH_BUFFER_BIT);
            return;
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
                let x = self.position.x + step.x;
                if !map::wall_blocked(self.map, x, self.position.z, feet, radius) {
                    self.position.x = x;
                }
                let z = self.position.z + step.z;
                if !map::wall_blocked(self.map, self.position.x, z, feet, radius) {
                    self.position.z = z;
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
            } else {
                self.position.y += self.vertical * dt;
            }
            if self.position.y < FALL_LIMIT {
                self.position = if self.map == Map::Camel {
                    Vec3::new(
                        map::CAMEL_SPAWN[0],
                        map::CAMEL_SPAWN[1] + map::EYE_HEIGHT,
                        map::CAMEL_SPAWN[2],
                    )
                } else {
                    Vec3::new(0.0, map::EYE_HEIGHT, 5.0)
                };
                self.vertical = 0.0;
            }
        }
        if self.screen == Screen::Playing && self.connected && now - self.last_sent >= 100.0 {
            if let Some(socket) = &self.socket {
                if socket.ready_state() == WebSocket::OPEN {
                    let _ = socket.send_with_str(&format!(
                        "POS|{:.3}|{:.3}|{:.3}|{:.3}",
                        self.position.x,
                        self.position.z,
                        self.position.y - map::EYE_HEIGHT,
                        self.yaw
                    ));
                    self.last_sent = now;
                }
            }
        }

        let direction = Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        );
        let projection = camera::rh::proj::opengl::perspective(
            70.0_f32.to_radians(),
            width as f32 / height.max(1) as f32,
            0.1,
            100.0,
        );
        let view = camera::rh::view::look_to_mat4(self.position, direction, Vec3::Y);
        let vp = projection * view;

        let gl = &self.gl;
        gl.clear_color(0.58, 0.77, 0.93, 1.0);
        gl.clear(Gl::COLOR_BUFFER_BIT | Gl::DEPTH_BUFFER_BIT);
        gl.enable(Gl::DEPTH_TEST);
        gl.use_program(Some(&self.program));
        gl.uniform1i(Some(&self.texture_enabled), 0);
        gl.bind_vertex_array(Some(&self.vao));
        for block in map::blocks(self.map) {
            let position = Vec3::from_array(block.center);
            let scale = Vec3::from_array(block.half);
            let model = Mat4::from_translation(position) * Mat4::from_scale(scale);
            gl.uniform_matrix4fv_with_f32_array(
                Some(&self.transform),
                false,
                &(vp * model).to_cols_array(),
            );
            gl.vertex_attrib4f(3, block.color[0], block.color[1], block.color[2], 1.0);
            gl.draw_elements_with_i32(Gl::TRIANGLES, 36, Gl::UNSIGNED_SHORT, 0);
        }
        if self.map == Map::Camel {
            gl.bind_vertex_array(Some(&self.camel_vao));
            let camel_vp = vp * Mat4::from_scale(Vec3::splat(map::CAMEL_SCALE));
            gl.uniform_matrix4fv_with_f32_array(
                Some(&self.transform),
                false,
                &camel_vp.to_cols_array(),
            );
            for part in &self.camel_parts {
                let texture = self.camel_textures[part.image].as_ref();
                gl.uniform1i(Some(&self.texture_enabled), i32::from(texture.is_some()));
                gl.vertex_attrib4f(3, 1.0, 1.0, 1.0, 1.0);
                if let Some(texture) = texture {
                    gl.bind_texture(Gl::TEXTURE_2D, Some(texture));
                }
                gl.draw_arrays(Gl::TRIANGLES, part.first, part.count);
            }
            gl.bind_vertex_array(Some(&self.vao));
        }
        gl.uniform1i(Some(&self.texture_enabled), 0);
        self.blood
            .retain(|effect| now - effect.at < if effect.death { 800.0 } else { 420.0 });
        if self.blood_input.checked() {
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
                    gl.uniform_matrix4fv_with_f32_array(
                        Some(&self.transform),
                        false,
                        &(vp * model).to_cols_array(),
                    );
                    gl.vertex_attrib4f(3, 0.55 + (index % 3) as f32 * 0.1, 0.02, 0.025, 1.0);
                    gl.draw_elements_with_i32(Gl::TRIANGLES, 36, Gl::UNSIGNED_SHORT, 0);
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
        gl.enable(Gl::BLEND);
        gl.blend_func(Gl::SRC_ALPHA, Gl::ONE_MINUS_SRC_ALPHA);
        gl.use_program(Some(&self.sprite_program));
        for player in players {
            let (texture, vaos, views) = match player.team {
                Team::Boys => (
                    &self.operative_texture,
                    &self.operative_vaos,
                    &OPERATIVE_VIEWS,
                ),
                Team::Girls => (&self.girl_texture, &self.girl_vaos, &GIRL_VIEWS),
            };
            let Some(texture) = texture else { continue };
            let view_index = operative_view(player, self.position);
            let crop = views[view_index];
            let half_width = 0.95 * (crop[2] - crop[0]) / (crop[3] - crop[1]);
            let facing_viewer = (self.position.x - player.x).atan2(self.position.z - player.z);
            let model = Mat4::from_translation(Vec3::new(player.x, player.y + 0.95, player.z))
                * Mat4::from_rotation_y(facing_viewer)
                * Mat4::from_scale(Vec3::new(half_width, 0.95, 1.0));
            gl.bind_texture(Gl::TEXTURE_2D, Some(texture));
            gl.bind_vertex_array(Some(&vaos[view_index]));
            gl.uniform_matrix4fv_with_f32_array(
                Some(&self.sprite_transform),
                false,
                &(vp * model).to_cols_array(),
            );
            gl.draw_arrays(Gl::TRIANGLES, 0, 6);
        }
        gl.disable(Gl::BLEND);
        gl.depth_mask(true);

        gl.disable(Gl::DEPTH_TEST);
        if self.blood_input.checked() && now - self.local_hit_at < 350.0 {
            let fade = (1.0 - (now - self.local_hit_at) as f32 / 350.0).clamp(0.0, 1.0);
            gl.enable(Gl::BLEND);
            gl.blend_func(Gl::SRC_ALPHA, Gl::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(&self.program));
            gl.bind_vertex_array(Some(&self.vao));
            for (x, y, size) in [
                (-0.91, 0.55, 0.09),
                (-0.88, -0.55, 0.06),
                (0.92, 0.35, 0.08),
                (0.84, -0.72, 0.07),
            ] {
                let size = size * if self.health == 0 { 1.8 } else { 1.0 };
                let model = Mat4::from_translation(Vec3::new(x, y, 0.0))
                    * Mat4::from_scale(Vec3::new(size, size * 1.4, 0.01));
                gl.uniform_matrix4fv_with_f32_array(
                    Some(&self.transform),
                    false,
                    &model.to_cols_array(),
                );
                gl.vertex_attrib4f(3, 0.7, 0.0, 0.02, fade * 0.6);
                gl.draw_elements_with_i32(Gl::TRIANGLES, 36, Gl::UNSIGNED_SHORT, 0);
            }
            gl.disable(Gl::BLEND);
        }
        let frame = if now - self.shot_at < 120.0 {
            1
        } else if now - self.shot_at < 240.0 {
            2
        } else {
            0
        };
        if let Some(texture) = &self.textures[frame] {
            let (aspect, sight_x) = match frame {
                0 => (1.5, 768.0 / 1536.0),
                1 => (1.0, 630.0 / 1254.0),
                _ => (1.0, 665.0 / 1254.0),
            };
            let sy = [0.65, 0.69, 0.66][frame];
            let sx = sy * aspect * height as f32 / width.max(1) as f32;
            let weapon =
                Mat4::from_translation(Vec3::new((0.5 - sight_x) * 2.0 * sx, -1.0 + sy, 0.0))
                    * Mat4::from_scale(Vec3::new(sx, sy, 1.0));
            gl.enable(Gl::BLEND);
            gl.blend_func(Gl::SRC_ALPHA, Gl::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(&self.sprite_program));
            gl.bind_vertex_array(Some(&self.sprite_vao));
            gl.bind_texture(Gl::TEXTURE_2D, Some(texture));
            gl.uniform_matrix4fv_with_f32_array(
                Some(&self.sprite_transform),
                false,
                &weapon.to_cols_array(),
            );
            gl.draw_arrays(Gl::TRIANGLES, 0, 6);
            gl.disable(Gl::BLEND);
        }
    }
}

fn send_join(game: &Rc<RefCell<Game>>) {
    let mut game = game.borrow_mut();
    let Some(team) = game.team else { return };
    if game.joined {
        return;
    }
    let Some(socket) = game.socket.clone() else {
        return;
    };
    if socket.ready_state() != WebSocket::OPEN {
        return;
    }
    let name = game.nickname_input.value().trim().to_owned();
    if socket
        .send_with_str(&format!("JOIN|{name}|{}", team.as_str()))
        .is_ok()
    {
        game.joined = true;
    }
}

fn connect(game: &Rc<RefCell<Game>>) -> Result<(), String> {
    let (address, name) = {
        let game = game.borrow();
        (
            game.server_input.value().trim().to_owned(),
            game.nickname_input.value().trim().to_owned(),
        )
    };
    if name.is_empty()
        || name.len() > 16
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err("Use 1–16 letters, numbers, _ or - for the nickname.".to_owned());
    }
    if address.is_empty() || address.chars().any(char::is_whitespace) {
        return Err("Enter a server address such as 127.0.0.1:9001.".to_owned());
    }
    let secure_page = web_sys::window()
        .unwrap()
        .location()
        .protocol()
        .unwrap_or_default()
        == "https:";
    let url = if address.starts_with("ws://") || address.starts_with("wss://") {
        address
    } else {
        format!("{}://{address}", if secure_page { "wss" } else { "ws" })
    };
    if secure_page && url.starts_with("ws://") {
        return Err("Use a wss:// server when the game page uses HTTPS.".to_owned());
    }
    let socket = WebSocket::new(&url).map_err(|_| "Invalid server address.".to_owned())?;
    let session = {
        let mut game = game.borrow_mut();
        game.disconnect();
        game.map = Map::Camel;
        game.yaw = -std::f32::consts::FRAC_PI_2;
        game.pitch = 0.0;
        game.vertical = 0.0;
        game.jump = false;
        game.socket = Some(socket.clone());
        game.last_sent = 0.0;
        game.joined = false;
        game.team = None;
        game.refresh_hud();
        game.show_screen(Screen::Team);
        game.session
    };

    let open_game = game.clone();
    let open = Closure::<dyn FnMut(Event)>::new(move |_| {
        send_join(&open_game);
    });
    socket.set_onopen(Some(open.as_ref().unchecked_ref()));
    open.forget();

    let message_game = game.clone();
    let message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let Some(text) = event.data().as_string() else {
            return;
        };
        let mut game = message_game.borrow_mut();
        if game.session != session {
            return;
        }
        let mut parts = text.split('|');
        match parts.next() {
            Some("WELCOME") => {
                if let (Some(id), Some(x), Some(z), Some(y), Some(health)) = (
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
                        game.position = Vec3::new(x, y + map::EYE_HEIGHT, z);
                        game.vertical = 0.0;
                        game.self_id = Some(id);
                        game.health = health;
                        game.refresh_health();
                        game.connected = true;
                        game.refresh_hud();
                    }
                }
            }
            Some("SCORE") => {
                if let (Some(Ok(boys)), Some(Ok(girls))) = (
                    parts.next().map(str::parse::<u32>),
                    parts.next().map(str::parse::<u32>),
                ) {
                    game.refresh_score(boys, girls);
                }
            }
            Some("WIN") => {
                if let Some(winner) = parts.next() {
                    let remaining = parts
                        .next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(10.0);
                    game.winner = Some(if winner == "girls" {
                        Team::Girls
                    } else {
                        Team::Boys
                    });
                    game.result_deadline = game.last_frame + remaining * 1000.0;
                    game.show_screen(Screen::Result);
                }
            }
            Some("AT") => {
                if let (Some(x), Some(y), Some(z)) = (parts.next(), parts.next(), parts.next()) {
                    if let (Ok(x), Ok(y), Ok(z)) =
                        (x.parse::<f32>(), y.parse::<f32>(), z.parse::<f32>())
                    {
                        game.position = Vec3::new(x, y + map::EYE_HEIGHT, z);
                        game.vertical = 0.0;
                        game.shot_at = f64::NEG_INFINITY;
                        // AT also starts a new round, so a past win must not block
                        // the death screen.
                        game.winner = None;
                        game.show_screen(Screen::Playing);
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
                    game.remote.insert(
                        id,
                        RemotePlayer {
                            name: name.to_owned(),
                            x,
                            z,
                            y,
                            yaw,
                            health,
                            team: if team == "girls" {
                                Team::Girls
                            } else {
                                Team::Boys
                            },
                        },
                    );
                    game.refresh_hud();
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
                    if let Some(player) = game.remote.get_mut(&id) {
                        player.x = x;
                        player.z = z;
                        player.y = y;
                        player.yaw = yaw;
                    }
                }
            }
            Some("LEAVE") => {
                if let Some(Ok(id)) = parts.next().map(str::parse::<u32>) {
                    game.remote.remove(&id);
                    game.refresh_hud();
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
                    if game.self_id == Some(id) {
                        if health < game.health {
                            hit_position = Some(Vec3::new(
                                game.position.x,
                                game.position.y - 0.6,
                                game.position.z,
                            ));
                            if game.blood_input.checked() {
                                game.local_hit_at = game.last_frame;
                            }
                        }
                        game.health = health;
                        game.refresh_health();
                        game.refresh_hud();
                        if health == 0 && game.winner.is_none() {
                            game.show_screen(Screen::Dead);
                        }
                    } else if let Some(player) = game.remote.get_mut(&id) {
                        if health < player.health {
                            hit_position = Some(Vec3::new(player.x, player.y + 1.1, player.z));
                        }
                        player.health = health;
                    }
                    if let Some(position) = hit_position {
                        if game.blood_input.checked() {
                            if game.blood.len() == 8 {
                                game.blood.remove(0);
                            }
                            let at = game.last_frame;
                            game.blood.push(BloodEffect {
                                position,
                                at,
                                death: health == 0,
                            });
                        }
                    }
                }
            }
            Some("ERROR") => {
                let reason = parts
                    .next()
                    .unwrap_or("Server rejected connection")
                    .to_owned();
                game.disconnect();
                game.status.set_inner_text(&reason);
                game.show_screen(Screen::Join);
            }
            _ => {}
        }
    });
    socket.set_onmessage(Some(message.as_ref().unchecked_ref()));
    message.forget();

    let close_game = game.clone();
    let close = Closure::<dyn FnMut(Event)>::new(move |_| {
        let mut game = close_game.borrow_mut();
        if game.session == session {
            game.disconnect();
            game.status
                .set_inner_text("Connection closed. Check the server address.");
            game.show_screen(Screen::Join);
        }
    });
    socket.set_onclose(Some(close.as_ref().unchecked_ref()));
    close.forget();
    Ok(())
}

#[wasm_bindgen(start)]
fn start() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or("No window")?;
    let document = window.document().ok_or("No document")?;
    document
        .body()
        .ok_or("No body")?
        .set_attribute("style", "margin:0;overflow:hidden")?;
    let canvas: HtmlCanvasElement = document
        .get_element_by_id("game")
        .ok_or("No canvas")?
        .dyn_into()?;
    canvas.set_attribute("style", "display:block")?;
    let gl: Gl = canvas
        .get_context("webgl2")?
        .ok_or("WebGL2 unavailable")?
        .dyn_into()?;

    let menu: HtmlElement = document.create_element("div")?.dyn_into()?;
    menu.set_attribute(
        "style",
        "position:fixed;inset:0;display:flex;align-items:center;justify-content:center;\
         background:rgba(0,0,0,.65);color:white;font:24px system-ui",
    )?;
    let panel: HtmlElement = document.create_element("div")?.dyn_into()?;
    panel.set_attribute(
        "style",
        "display:flex;flex-direction:column;gap:12px;min-width:240px;text-align:center",
    )?;
    let menu_title: HtmlElement = document.create_element("h1")?.dyn_into()?;
    menu_title.set_attribute("style", "font:36px system-ui;margin:0 0 16px")?;
    panel.append_child(&menu_title)?;
    let server_input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
    server_input.set_attribute("aria-label", "Server address")?;
    server_input.set_attribute("placeholder", "Server IP:port or wss:// address")?;
    server_input.set_attribute(
        "style",
        "font:18px system-ui;padding:10px;box-sizing:border-box;width:100%",
    )?;
    server_input.set_value("127.0.0.1:9001");
    panel.append_child(&server_input)?;
    let nickname_input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
    nickname_input.set_attribute("aria-label", "Nickname")?;
    nickname_input.set_attribute("placeholder", "Nickname")?;
    nickname_input.set_attribute("maxlength", "16")?;
    nickname_input.set_attribute(
        "style",
        "font:18px system-ui;padding:10px;box-sizing:border-box;width:100%",
    )?;
    panel.append_child(&nickname_input)?;
    let status: HtmlElement = document.create_element("div")?.dyn_into()?;
    status.set_attribute("style", "font:16px system-ui;max-width:300px")?;
    panel.append_child(&status)?;
    let settings_panel: HtmlElement = document.create_element("div")?.dyn_into()?;
    settings_panel.set_attribute(
        "style",
        "display:none;flex-direction:column;gap:8px;text-align:left;font:18px system-ui",
    )?;
    let sensitivity_label: HtmlElement = document.create_element("label")?.dyn_into()?;
    sensitivity_label.set_attribute("for", "sensitivity")?;
    sensitivity_label.set_inner_text("Sensitivity: 100% (Minecraft Java)");
    settings_panel.append_child(&sensitivity_label)?;
    let sensitivity_input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
    sensitivity_input.set_type("range");
    sensitivity_input.set_id("sensitivity");
    sensitivity_input.set_min("0");
    sensitivity_input.set_max("200");
    sensitivity_input.set_step("1");
    sensitivity_input.set_value("100");
    settings_panel.append_child(&sensitivity_input)?;
    let raw_label: HtmlElement = document.create_element("label")?.dyn_into()?;
    raw_label.set_attribute("style", "display:flex;justify-content:space-between")?;
    raw_label.set_inner_text("Raw input");
    let raw_input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
    raw_input.set_type("checkbox");
    raw_input.set_checked(false);
    raw_label.append_child(&raw_input)?;
    settings_panel.append_child(&raw_label)?;
    let hud_scale_label: HtmlElement = document.create_element("label")?.dyn_into()?;
    hud_scale_label.set_attribute("for", "hud-scale")?;
    hud_scale_label.set_inner_text("HUD scale: 100%");
    settings_panel.append_child(&hud_scale_label)?;
    let hud_scale_input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
    hud_scale_input.set_type("range");
    hud_scale_input.set_id("hud-scale");
    hud_scale_input.set_min("50");
    hud_scale_input.set_max("200");
    hud_scale_input.set_step("10");
    hud_scale_input.set_value("100");
    settings_panel.append_child(&hud_scale_input)?;
    let fps_label: HtmlElement = document.create_element("label")?.dyn_into()?;
    fps_label.set_attribute("style", "display:flex;justify-content:space-between")?;
    fps_label.set_inner_text("Show FPS");
    let fps_input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
    fps_input.set_type("checkbox");
    fps_input.set_checked(true);
    fps_label.append_child(&fps_input)?;
    settings_panel.append_child(&fps_label)?;
    let blood_label: HtmlElement = document.create_element("label")?.dyn_into()?;
    blood_label.set_attribute("style", "display:flex;justify-content:space-between")?;
    blood_label.set_inner_text("Show blood effects");
    let blood_input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
    blood_input.set_type("checkbox");
    blood_input.set_checked(true);
    blood_label.append_child(&blood_input)?;
    settings_panel.append_child(&blood_label)?;
    panel.append_child(&settings_panel)?;
    let mut menu_buttons = Vec::new();
    for _ in 0..3 {
        let button: HtmlButtonElement = document.create_element("button")?.dyn_into()?;
        button.set_attribute(
            "style",
            "font:inherit;padding:12px;background:#242a35;color:white;\
             border:1px solid #889;cursor:pointer",
        )?;
        panel.append_child(&button)?;
        menu_buttons.push(button);
    }
    menu.append_child(&panel)?;
    document.body().unwrap().append_child(&menu)?;
    let hud: HtmlElement = document.create_element("div")?.dyn_into()?;
    hud.set_attribute(
        "style",
        "position:fixed;top:12px;left:12px;white-space:pre-line;\
         color:white;text-shadow:1px 1px 2px black;font:16px system-ui",
    )?;
    document.body().unwrap().append_child(&hud)?;
    let fps_hud: HtmlElement = document.create_element("div")?.dyn_into()?;
    fps_hud.set_attribute(
        "style",
        "position:fixed;top:12px;right:12px;color:white;\
         text-shadow:1px 1px 2px black;font:16px system-ui",
    )?;
    document.body().unwrap().append_child(&fps_hud)?;
    let score_hud: HtmlElement = document.create_element("div")?.dyn_into()?;
    score_hud.set_attribute(
        "style",
        "position:fixed;top:12px;left:50%;transform:translateX(-50%);display:flex;\
         gap:28px;font:20px system-ui;font-weight:bold;text-shadow:1px 1px 2px black",
    )?;
    let boys_score: HtmlElement = document.create_element("div")?.dyn_into()?;
    score_hud.append_child(&boys_score)?;
    let girls_score: HtmlElement = document.create_element("div")?.dyn_into()?;
    score_hud.append_child(&girls_score)?;
    document.body().unwrap().append_child(&score_hud)?;
    let health_hud: HtmlElement = document.create_element("div")?.dyn_into()?;
    health_hud.set_attribute(
        "style",
        "position:fixed;bottom:18px;left:18px;color:white;\
         text-shadow:1px 1px 2px black;font:18px system-ui",
    )?;
    let health_label: HtmlElement = document.create_element("div")?.dyn_into()?;
    health_hud.append_child(&health_label)?;
    let health_back: HtmlElement = document.create_element("div")?.dyn_into()?;
    health_back.set_attribute(
        "style",
        "width:180px;height:14px;margin-top:5px;background:#303030;border:1px solid white",
    )?;
    let health_fill: HtmlElement = document.create_element("div")?.dyn_into()?;
    health_fill.set_attribute("style", "height:100%;background:#40c44c")?;
    health_back.append_child(&health_fill)?;
    health_hud.append_child(&health_back)?;
    document.body().unwrap().append_child(&health_hud)?;

    let vertex = compile_shader(&gl, Gl::VERTEX_SHADER, VERTEX_SHADER)?;
    let fragment = compile_shader(&gl, Gl::FRAGMENT_SHADER, FRAGMENT_SHADER)?;
    let program = link_program(&gl, &vertex, &fragment)?;
    gl.delete_shader(Some(&vertex));
    gl.delete_shader(Some(&fragment));
    let transform = gl
        .get_uniform_location(&program, "MVP")
        .ok_or("No MVP uniform")?;
    let texture_enabled = gl
        .get_uniform_location(&program, "use_texture")
        .ok_or("No texture switch")?;
    let sprite_vertex = compile_shader(&gl, Gl::VERTEX_SHADER, SPRITE_VERTEX_SHADER)?;
    let sprite_fragment = compile_shader(&gl, Gl::FRAGMENT_SHADER, SPRITE_FRAGMENT_SHADER)?;
    let sprite_program = link_program(&gl, &sprite_vertex, &sprite_fragment)?;
    gl.delete_shader(Some(&sprite_vertex));
    gl.delete_shader(Some(&sprite_fragment));
    let sprite_transform = gl
        .get_uniform_location(&sprite_program, "MVP")
        .ok_or("No sprite MVP uniform")?;

    let vao = gl.create_vertex_array().ok_or("No vertex array")?;
    gl.bind_vertex_array(Some(&vao));
    let vertices = gl.create_buffer().ok_or("No vertex buffer")?;
    gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&vertices));
    gl.buffer_data_with_array_buffer_view(
        Gl::ARRAY_BUFFER,
        &js_sys::Float32Array::from(VERTICES.as_slice()),
        Gl::STATIC_DRAW,
    );
    gl.vertex_attrib_pointer_with_i32(0, 3, Gl::FLOAT, false, 0, 0);
    gl.enable_vertex_attrib_array(0);
    let indices = gl.create_buffer().ok_or("No index buffer")?;
    gl.bind_buffer(Gl::ELEMENT_ARRAY_BUFFER, Some(&indices));
    gl.buffer_data_with_array_buffer_view(
        Gl::ELEMENT_ARRAY_BUFFER,
        &js_sys::Uint16Array::from(INDICES.as_slice()),
        Gl::STATIC_DRAW,
    );
    // Mesh extracted from the user's Sketchfab GLB by tools/convert (CC BY 4.0).
    let camel_vao = gl.create_vertex_array().ok_or("No Camel vertex array")?;
    gl.bind_vertex_array(Some(&camel_vao));
    let camel_buffer = gl.create_buffer().ok_or("No Camel vertex buffer")?;
    gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&camel_buffer));
    gl.buffer_data_with_array_buffer_view(
        Gl::ARRAY_BUFFER,
        &js_sys::Uint8Array::from(include_bytes!("../assets/camel_mesh.bin").as_slice()),
        Gl::STATIC_DRAW,
    );
    gl.vertex_attrib_pointer_with_i32(0, 3, Gl::FLOAT, false, 20, 0);
    gl.enable_vertex_attrib_array(0);
    gl.vertex_attrib_pointer_with_i32(4, 2, Gl::FLOAT, false, 20, 12);
    gl.enable_vertex_attrib_array(4);
    let camel_parts: Vec<CamelPart> = include_str!("../assets/camel_parts.txt")
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
    let sprite_vao = gl.create_vertex_array().ok_or("No sprite vertex array")?;
    gl.bind_vertex_array(Some(&sprite_vao));
    let sprite_vertices = gl.create_buffer().ok_or("No sprite vertex buffer")?;
    gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&sprite_vertices));
    gl.buffer_data_with_array_buffer_view(
        Gl::ARRAY_BUFFER,
        &js_sys::Float32Array::from(SPRITE_VERTICES.as_slice()),
        Gl::STATIC_DRAW,
    );
    gl.vertex_attrib_pointer_with_i32(0, 2, Gl::FLOAT, false, 16, 0);
    gl.enable_vertex_attrib_array(0);
    gl.vertex_attrib_pointer_with_i32(4, 2, Gl::FLOAT, false, 16, 8);
    gl.enable_vertex_attrib_array(4);

    let operative_vaos = build_operative_vaos(&gl, &OPERATIVE_VIEWS, 1536.0, 1024.0)?;
    let girl_vaos = build_operative_vaos(&gl, &GIRL_VIEWS, 1448.0, 1086.0)?;

    let game = Rc::new(RefCell::new(Game {
        gl,
        canvas: canvas.clone(),
        program,
        vao,
        camel_vao,
        camel_parts,
        camel_textures: vec![None; 34],
        transform,
        texture_enabled,
        sprite_program,
        sprite_vao,
        operative_vaos,
        girl_vaos,
        sprite_transform,
        textures: [None, None, None],
        operative_texture: None,
        girl_texture: None,
        shot_at: f64::NEG_INFINITY,
        position: Vec3::new(0.0, 1.7, 5.0),
        yaw: -std::f32::consts::FRAC_PI_2,
        pitch: 0.0,
        keys: [false; 4],
        jump: false,
        vertical: 0.0,
        last_frame: 0.0,
        screen: Screen::Main,
        map: Map::Test,
        menu,
        menu_title,
        menu_buttons,
        server_input,
        nickname_input,
        status,
        settings_panel,
        sensitivity_input,
        raw_input,
        hud_scale_input,
        fps_input,
        blood_input,
        hud,
        fps_hud,
        fps_since: 0.0,
        fps_frames: 0,
        score_hud,
        boys_score,
        girls_score,
        health_hud,
        health_label,
        health_fill,
        health: 3,
        self_id: None,
        socket: None,
        connected: false,
        joined: false,
        team: None,
        winner: None,
        result_deadline: 0.0,
        session: 0,
        last_sent: 0.0,
        remote: HashMap::new(),
        blood: Vec::new(),
        local_hit_at: f64::NEG_INFINITY,
    }));
    game.borrow().refresh_health();
    game.borrow().refresh_hud_scale();
    game.borrow_mut().show_screen(Screen::Main);

    let sensitivity_value = sensitivity_label.clone();
    let sensitivity_slider = game.borrow().sensitivity_input.clone();
    let sensitivity_changed = Closure::<dyn FnMut(Event)>::new(move |_| {
        sensitivity_value.set_inner_text(&format!(
            "Sensitivity: {}% (Minecraft Java)",
            sensitivity_slider.value()
        ));
    });
    game.borrow()
        .sensitivity_input
        .add_event_listener_with_callback("input", sensitivity_changed.as_ref().unchecked_ref())?;
    sensitivity_changed.forget();
    let scale_game = game.clone();
    let scale_changed = Closure::<dyn FnMut(Event)>::new(move |_| {
        let game = scale_game.borrow();
        hud_scale_label.set_inner_text(&format!("HUD scale: {}%", game.hud_scale_input.value()));
        game.refresh_hud_scale();
    });
    game.borrow()
        .hud_scale_input
        .add_event_listener_with_callback("input", scale_changed.as_ref().unchecked_ref())?;
    scale_changed.forget();
    let fps_game = game.clone();
    let fps_changed = Closure::<dyn FnMut(Event)>::new(move |_| {
        fps_game.borrow().refresh_fps_visibility();
    });
    game.borrow()
        .fps_input
        .add_event_listener_with_callback("change", fps_changed.as_ref().unchecked_ref())?;
    fps_changed.forget();
    let blood_game = game.clone();
    let blood_changed = Closure::<dyn FnMut(Event)>::new(move |_| {
        let mut game = blood_game.borrow_mut();
        if !game.blood_input.checked() {
            game.blood.clear();
            game.local_hit_at = f64::NEG_INFINITY;
        }
    });
    game.borrow()
        .blood_input
        .add_event_listener_with_callback("change", blood_changed.as_ref().unchecked_ref())?;
    blood_changed.forget();

    for (index, path) in [
        "assets/weapon_idle.png",
        "assets/weapon_flash.png",
        "assets/weapon_recoil.png",
    ]
    .into_iter()
    .enumerate()
    {
        let image = HtmlImageElement::new()?;
        let loaded_image = image.clone();
        let loaded_game = game.clone();
        let load = Closure::<dyn FnMut()>::new(move || {
            let mut game = loaded_game.borrow_mut();
            let gl = &game.gl;
            if let Some(texture) = gl.create_texture() {
                gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
                gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MIN_FILTER, Gl::LINEAR as i32);
                gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::LINEAR as i32);
                gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE as i32);
                gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE as i32);
                if gl
                    .tex_image_2d_with_u32_and_u32_and_html_image_element(
                        Gl::TEXTURE_2D,
                        0,
                        Gl::RGBA as i32,
                        Gl::RGBA,
                        Gl::UNSIGNED_BYTE,
                        &loaded_image,
                    )
                    .is_ok()
                {
                    game.textures[index] = Some(texture);
                }
            }
        });
        image.add_event_listener_with_callback("load", load.as_ref().unchecked_ref())?;
        load.forget();
        image.set_src(path);
    }

    load_texture(
        game.clone(),
        "assets/operative_turnaround.png",
        |game, texture| {
            game.operative_texture = Some(texture);
        },
    )?;
    load_texture(
        game.clone(),
        "assets/girl_turnaround.png",
        |game, texture| {
            game.girl_texture = Some(texture);
        },
    )?;

    // Original CC BY 4.0 images embedded in the supplied Sketchfab GLB.
    for index in 0..34 {
        let image = HtmlImageElement::new()?;
        let loaded_image = image.clone();
        let loaded_game = game.clone();
        let load = Closure::<dyn FnMut()>::new(move || {
            let mut game = loaded_game.borrow_mut();
            let gl = &game.gl;
            if let Some(texture) = gl.create_texture() {
                gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
                gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MIN_FILTER, Gl::LINEAR as i32);
                gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::LINEAR as i32);
                gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, Gl::REPEAT as i32);
                gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::REPEAT as i32);
                if gl
                    .tex_image_2d_with_u32_and_u32_and_html_image_element(
                        Gl::TEXTURE_2D,
                        0,
                        Gl::RGBA as i32,
                        Gl::RGBA,
                        Gl::UNSIGNED_BYTE,
                        &loaded_image,
                    )
                    .is_ok()
                {
                    game.camel_textures[index] = Some(texture);
                }
            }
        });
        image.add_event_listener_with_callback("load", load.as_ref().unchecked_ref())?;
        load.forget();
        image.set_src(&format!("assets/camel_images/{index}.png"));
    }

    let lock_canvas = canvas.clone();
    let lock_document = document.clone();
    let lock_game = game.clone();
    let denied_game = game.clone();
    let denied_document = document.clone();
    let pointer_lock_denied = Closure::<dyn FnMut(JsValue)>::new(move |_| {
        denied_game.borrow_mut().pointer_lock_failed();
        denied_document.exit_pointer_lock();
    });
    let lock_pointer = Rc::new(move || {
        if lock_document.pointer_lock_element().is_some() {
            return;
        }
        let raw = lock_game.borrow().raw_input.checked();
        // web-sys exposes only the older no-options signature. Call the browser
        // method from Rust so we can pass PointerLockOptions when raw is checked.
        // https://www.w3.org/TR/pointerlock/#dom-element-requestpointerlock
        if let Ok(method) = js_sys::Reflect::get(lock_canvas.as_ref(), &"requestPointerLock".into())
        {
            if let Ok(method) = method.dyn_into::<js_sys::Function>() {
                let result = if raw {
                    let options = js_sys::Object::new();
                    let _ =
                        js_sys::Reflect::set(&options, &"unadjustedMovement".into(), &true.into());
                    method.call1(lock_canvas.as_ref(), &options)
                } else {
                    method.call0(lock_canvas.as_ref())
                };
                match result {
                    Ok(result) => {
                        if let Ok(promise) = result.dyn_into::<js_sys::Promise>() {
                            let _ = promise.catch(&pointer_lock_denied);
                        } else if raw {
                            lock_document.exit_pointer_lock();
                            lock_game.borrow_mut().pointer_lock_failed();
                        }
                    }
                    Err(_) => {
                        lock_document.exit_pointer_lock();
                        lock_game.borrow_mut().pointer_lock_failed();
                    }
                }
            } else {
                lock_game.borrow_mut().pointer_lock_failed();
            }
        } else {
            lock_game.borrow_mut().pointer_lock_failed();
        }
    });
    for index in 0..3 {
        let button = game.borrow().menu_buttons[index].clone();
        let button_game = game.clone();
        let button_lock = lock_pointer.clone();
        let action = Closure::<dyn FnMut()>::new(move || {
            if button_game.borrow().screen == Screen::Join && index == 0 {
                if let Err(error) = connect(&button_game) {
                    button_game.borrow().status.set_inner_text(&error);
                }
                return;
            }
            if button_game.borrow().screen == Screen::Team && index < 2 {
                {
                    let mut game = button_game.borrow_mut();
                    game.team = Some(if index == 0 { Team::Boys } else { Team::Girls });
                    game.show_screen(Screen::Playing);
                }
                send_join(&button_game);
                button_lock();
                return;
            }
            if button_game.borrow().screen == Screen::Dead && index == 0 {
                if let Some(socket) = button_game.borrow().socket.clone() {
                    let _ = socket.send_with_str("RESPAWN");
                }
                button_game.borrow_mut().show_screen(Screen::Playing);
                button_lock();
                return;
            }
            let mut game = button_game.borrow_mut();
            let next = match (game.screen, index) {
                (Screen::Main, 0) => Screen::Modes,
                (Screen::Main, 1) => Screen::SettingsMain,
                (Screen::Modes, 0) => {
                    game.status.set_inner_text("");
                    Screen::Join
                }
                (Screen::Modes, 1) => Screen::MapSelect,
                (Screen::MapSelect, 0 | 1) => {
                    game.disconnect();
                    game.map = if index == 0 { Map::Camel } else { Map::Test };
                    game.position = if index == 0 {
                        Vec3::new(
                            map::CAMEL_SPAWN[0],
                            map::CAMEL_SPAWN[1] + 1.7,
                            map::CAMEL_SPAWN[2],
                        )
                    } else {
                        Vec3::new(0.0, 1.7, 5.0)
                    };
                    game.yaw = -std::f32::consts::FRAC_PI_2;
                    game.pitch = 0.0;
                    game.vertical = 0.0;
                    game.jump = false;
                    game.shot_at = f64::NEG_INFINITY;
                    game.health = 3;
                    game.refresh_health();
                    Screen::Playing
                }
                (Screen::MapSelect, 2) => Screen::Modes,
                (Screen::Join, 1) => Screen::Modes,
                (Screen::Paused, 0) => Screen::Playing,
                (Screen::Paused, 1) => Screen::SettingsPaused,
                (Screen::Paused, 2) => {
                    game.disconnect();
                    Screen::Main
                }
                (Screen::SettingsMain, 0) => Screen::Main,
                (Screen::SettingsPaused, 0) => Screen::Paused,
                _ => return,
            };
            game.show_screen(next);
            drop(game);
            if next == Screen::Playing {
                button_lock();
            }
        });
        button.add_event_listener_with_callback("click", action.as_ref().unchecked_ref())?;
        action.forget();
    }

    let click_game = game.clone();
    let click_lock = lock_pointer.clone();
    let click = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
        if event.button() != 0
            || click_game.borrow().screen != Screen::Playing
            || click_game.borrow().health == 0
        {
            return;
        }
        let mut game = click_game.borrow_mut();
        if game.last_frame - game.shot_at < FIRE_COOLDOWN_MS {
            drop(game);
            click_lock();
            return;
        }
        game.shot_at = game.last_frame;
        if game.connected {
            if let Some(socket) = &game.socket {
                let _ = socket.send_with_str(&format!(
                    "FIRE|{:.3}|{:.3}|{:.5}|{:.5}",
                    game.position.x, game.position.z, game.yaw, game.pitch
                ));
            }
        }
        drop(game);
        click_lock();
    });
    canvas.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
    click.forget();

    let mouse_game = game.clone();
    let mouse_document = document.clone();
    let mouse = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
        let mut game = mouse_game.borrow_mut();
        if mouse_document.pointer_lock_element().is_some() && game.screen == Screen::Playing {
            let radians =
                minecraft_radians_per_count(game.sensitivity_input.value_as_number() as f32);
            game.yaw += event.movement_x() as f32 * radians;
            game.pitch = (game.pitch - event.movement_y() as f32 * radians).clamp(-1.5, 1.5);
        }
    });
    document.add_event_listener_with_callback("mousemove", mouse.as_ref().unchecked_ref())?;
    mouse.forget();

    let lock_game = game.clone();
    let lock_document = document.clone();
    let pointer_change = Closure::<dyn FnMut()>::new(move || {
        let mut game = lock_game.borrow_mut();
        if lock_document.pointer_lock_element().is_some() && game.screen != Screen::Playing {
            lock_document.exit_pointer_lock();
        } else if lock_document.pointer_lock_element().is_none() && game.screen == Screen::Playing {
            game.show_screen(Screen::Paused);
        }
    });
    document.add_event_listener_with_callback(
        "pointerlockchange",
        pointer_change.as_ref().unchecked_ref(),
    )?;
    pointer_change.forget();
    let error_game = game.clone();
    let pointer_error = Closure::<dyn FnMut(Event)>::new(move |_| {
        error_game.borrow_mut().pointer_lock_failed();
    });
    document.add_event_listener_with_callback(
        "pointerlockerror",
        pointer_error.as_ref().unchecked_ref(),
    )?;
    pointer_error.forget();

    for (kind, pressed) in [("keydown", true), ("keyup", false)] {
        let key_game = game.clone();
        let key_document = document.clone();
        let key = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
            if event.code() == "Escape" && pressed {
                let mut game = key_game.borrow_mut();
                match game.screen {
                    Screen::Playing => {
                        event.prevent_default();
                        game.show_screen(Screen::Paused);
                        key_document.exit_pointer_lock();
                    }
                    Screen::Modes => game.show_screen(Screen::Main),
                    Screen::MapSelect => game.show_screen(Screen::Modes),
                    Screen::Join => game.show_screen(Screen::Modes),
                    Screen::Team => {
                        game.disconnect();
                        game.show_screen(Screen::Join);
                    }
                    Screen::Dead | Screen::Result => {
                        game.show_screen(Screen::Paused);
                        key_document.exit_pointer_lock();
                    }
                    Screen::SettingsMain => game.show_screen(Screen::Main),
                    Screen::SettingsPaused => game.show_screen(Screen::Paused),
                    _ => {}
                }
                return;
            }
            if event.code() == "Space" && !event.repeat() {
                let mut game = key_game.borrow_mut();
                if game.screen == Screen::Playing {
                    event.prevent_default();
                    game.jump = pressed;
                }
                return;
            }
            let index = match event.code().as_str() {
                "KeyW" => Some(0),
                "KeyA" => Some(1),
                "KeyS" => Some(2),
                "KeyD" => Some(3),
                _ => None,
            };
            if let Some(index) = index {
                let mut game = key_game.borrow_mut();
                if game.screen == Screen::Playing {
                    event.prevent_default();
                    game.keys[index] = pressed;
                }
            }
        });
        document.add_event_listener_with_callback(kind, key.as_ref().unchecked_ref())?;
        key.forget();
    }

    let frames: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let next = frames.clone();
    *next.borrow_mut() = Some(Closure::new(move |now| {
        game.borrow_mut().draw(now);
        web_sys::window()
            .unwrap()
            .request_animation_frame(frames.borrow().as_ref().unwrap().as_ref().unchecked_ref())
            .unwrap();
    }));
    window.request_animation_frame(next.borrow().as_ref().unwrap().as_ref().unchecked_ref())?;
    Ok(())
}

fn load_texture(
    game: Rc<RefCell<Game>>,
    src: &str,
    assign: fn(&mut Game, WebGlTexture),
) -> Result<(), JsValue> {
    let image = HtmlImageElement::new()?;
    let loaded_image = image.clone();
    let loaded_game = game.clone();
    let load = Closure::<dyn FnMut()>::new(move || {
        let mut game = loaded_game.borrow_mut();
        let gl = &game.gl;
        if let Some(texture) = gl.create_texture() {
            gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
            gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MIN_FILTER, Gl::LINEAR as i32);
            gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::LINEAR as i32);
            gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE as i32);
            gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE as i32);
            if gl
                .tex_image_2d_with_u32_and_u32_and_html_image_element(
                    Gl::TEXTURE_2D,
                    0,
                    Gl::RGBA as i32,
                    Gl::RGBA,
                    Gl::UNSIGNED_BYTE,
                    &loaded_image,
                )
                .is_ok()
            {
                assign(&mut game, texture);
            }
        }
    });
    image.add_event_listener_with_callback("load", load.as_ref().unchecked_ref())?;
    load.forget();
    image.set_src(src);
    Ok(())
}

fn build_operative_vaos(
    gl: &Gl,
    views: &[[f32; 4]; 4],
    width: f32,
    height: f32,
) -> Result<Vec<WebGlVertexArrayObject>, JsValue> {
    let mut vaos = Vec::new();
    for &[left, top, right, bottom] in views {
        let (u0, v0, u1, v1) = (left / width, top / height, right / width, bottom / height);
        let quad = [
            -1.0, -1.0, u0, v1, 1.0, -1.0, u1, v1, 1.0, 1.0, u1, v0, -1.0, -1.0, u0, v1, 1.0, 1.0,
            u1, v0, -1.0, 1.0, u0, v0,
        ];
        let vao = gl
            .create_vertex_array()
            .ok_or("No operative vertex array")?;
        gl.bind_vertex_array(Some(&vao));
        let buffer = gl.create_buffer().ok_or("No operative vertex buffer")?;
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&buffer));
        gl.buffer_data_with_array_buffer_view(
            Gl::ARRAY_BUFFER,
            &js_sys::Float32Array::from(quad.as_slice()),
            Gl::STATIC_DRAW,
        );
        gl.vertex_attrib_pointer_with_i32(0, 2, Gl::FLOAT, false, 16, 0);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_with_i32(4, 2, Gl::FLOAT, false, 16, 8);
        gl.enable_vertex_attrib_array(4);
        vaos.push(vao);
    }
    Ok(vaos)
}

// Adapted from wasm-bindgen/examples/webgl/src/lib.rs, lines 89-134.
// https://github.com/wasm-bindgen/wasm-bindgen/blob/main/examples/webgl/src/lib.rs
// License: MIT (see LICENSE-MIT-wasm-bindgen). Keeps compile and link error logs visible.
fn compile_shader(gl: &Gl, kind: u32, source: &str) -> Result<WebGlShader, JsValue> {
    let shader = gl.create_shader(kind).ok_or("Could not create shader")?;
    gl.shader_source(&shader, source);
    gl.compile_shader(&shader);
    if gl
        .get_shader_parameter(&shader, Gl::COMPILE_STATUS)
        .as_bool()
        == Some(true)
    {
        Ok(shader)
    } else {
        Err(JsValue::from_str(
            &gl.get_shader_info_log(&shader).unwrap_or_default(),
        ))
    }
}

fn link_program(
    gl: &Gl,
    vertex: &WebGlShader,
    fragment: &WebGlShader,
) -> Result<WebGlProgram, JsValue> {
    let program = gl.create_program().ok_or("Could not create program")?;
    gl.attach_shader(&program, vertex);
    gl.attach_shader(&program, fragment);
    gl.link_program(&program);
    if gl
        .get_program_parameter(&program, Gl::LINK_STATUS)
        .as_bool()
        == Some(true)
    {
        Ok(program)
    } else {
        Err(JsValue::from_str(
            &gl.get_program_info_log(&program).unwrap_or_default(),
        ))
    }
}
