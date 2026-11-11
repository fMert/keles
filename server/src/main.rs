use std::{
    collections::HashMap,
    net::{IpAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungstenite::{accept_hdr_with_config, protocol::WebSocketConfig, Error, Message};
// Shared with the client; each side uses only part of it.
mod accounts;
#[allow(dead_code)]
#[path = "../../src/map.rs"]
mod map;
use accounts::Account;

const MAX_PLAYERS: usize = 16;
// Leave a few bounded handshake slots so full matches can return ERROR.
const MAX_CONNECTIONS: usize = 160;
// ponytail: players behind one NAT share this; raise it if they do. Handshakes
// that never JOIN are bounded by MAX_CONNECTIONS threads and the 5 s timeout.
const MAX_PER_IP: usize = 4;
const MAX_HEALTH: u8 = 3;
const ROUND_RESET: Duration = Duration::from_secs(10);

struct Player {
    key: String,
    playing: bool,
    name: String,
    x: f32,
    z: f32,
    y: f32,
    yaw: f32,
    health: u8,
    team: &'static str,
    ip: IpAddr,
    outgoing: SyncSender<String>,
}

struct World {
    players: HashMap<u32, Player>,
    boys: u32,
    girls: u32,
    winner: Option<&'static str>,
    reset_at: Option<Instant>,
    accounts: HashMap<String, Account>,
    sessions: HashMap<String, String>,
    auth_attempts: HashMap<IpAddr, (Instant, u32)>,
    registrations: HashMap<IpAddr, (Instant, u32)>,
    data_path: PathBuf,
}

impl World {
    fn new(data_path: PathBuf) -> std::io::Result<Self> {
        Ok(Self {
            players: HashMap::new(),
            boys: 0,
            girls: 0,
            winner: None,
            reset_at: None,
            accounts: accounts::load(&data_path)?,
            sessions: HashMap::new(),
            auth_attempts: HashMap::new(),
            registrations: HashMap::new(),
            data_path,
        })
    }
}

fn limited(
    entries: &mut HashMap<IpAddr, (Instant, u32)>,
    ip: IpAddr,
    seconds: u64,
    limit: u32,
) -> bool {
    entries.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(seconds));
    let (_, count) = entries.entry(ip).or_insert((Instant::now(), 0));
    *count += 1;
    *count > limit
}

// Hashing is deliberately outside the World lock, including the unknown-user path.
fn authenticate(
    shared: &Shared,
    ip: IpAddr,
    text: &str,
    id: u32,
    outgoing: SyncSender<String>,
) -> Result<String, &'static str> {
    let mut fields = text.splitn(3, '|');
    let kind = fields.next().unwrap_or_default();
    let name = fields.next().unwrap_or_default();
    let password = fields.next().unwrap_or_default();
    {
        let mut world = shared.lock().unwrap();
        if limited(&mut world.auth_attempts, ip, 60, 10)
            || (kind == "REGISTER" && limited(&mut world.registrations, ip, 3600, 3))
        {
            return Err("rate_limited");
        }
    }
    let (key, new_hash) = if kind == "TOKEN" {
        if name.len() != 64 || !name.bytes().all(|b| b.is_ascii_hexdigit()) || !password.is_empty()
        {
            return Err("expired");
        }
        let world = shared.lock().unwrap();
        (world.sessions.get(name).cloned().ok_or("expired")?, None)
    } else {
        if !matches!(kind, "LOGIN" | "REGISTER") {
            return Err("wrong");
        }
        if !accounts::valid_username(name) {
            return Err("bad_username");
        }
        if !accounts::valid_password(password) {
            return Err("bad_password");
        }
        let key = name.to_ascii_lowercase();
        if kind == "REGISTER" {
            (key, Some(accounts::hash(password)?))
        } else {
            let stored = shared
                .lock()
                .unwrap()
                .accounts
                .get(&key)
                .map(|a| a.hash.clone());
            // The same Argon2 cost hides whether the name exists.
            let correct = match stored {
                Some(hash) => accounts::verify(password, &hash),
                None => {
                    let _ = accounts::hash(password)?;
                    false
                }
            };
            if !correct {
                return Err("wrong");
            }
            (key, None)
        }
    };
    let mut world = shared.lock().unwrap();
    if kind == "TOKEN" && world.sessions.get(name) != Some(&key) {
        return Err("expired");
    }
    if new_hash.is_some() && world.accounts.contains_key(&key) {
        return Err("taken");
    }
    if world.players.values().any(|p| p.key == key) {
        return Err("online");
    }
    if world.players.len() >= MAX_CONNECTIONS
        || world.players.values().filter(|p| p.ip == ip).count() >= MAX_PER_IP
    {
        return Err("full");
    }
    if let Some(hash) = new_hash {
        world.accounts.insert(
            key.clone(),
            Account {
                name: name.to_owned(),
                hash,
                rating: 1000,
                wins: 0,
                losses: 0,
            },
        );
        if let Err(error) = accounts::save(&world.data_path, &world.accounts) {
            world.accounts.remove(&key);
            eprintln!("Hesap kaydedilemedi: {error}");
            return Err("full");
        }
    }
    let token = if kind == "TOKEN" {
        name.to_owned()
    } else {
        world.sessions.retain(|_, owner| owner != &key);
        let token = accounts::token();
        world.sessions.insert(token.clone(), key.clone());
        token
    };
    let a = &world.accounts[&key];
    let response = format!(
        "AUTH_OK|{}|{token}|{}|{}|{}",
        a.name, a.rating, a.wins, a.losses
    );
    let name = a.name.clone();
    world.players.insert(
        id,
        Player {
            key,
            name,
            playing: false,
            x: 0.0,
            z: 0.0,
            y: 0.0,
            yaw: 0.0,
            health: MAX_HEALTH,
            team: "boys",
            ip,
            outgoing,
        },
    );
    Ok(response)
}

fn info(world: &World, id: u32) {
    let Some(player) = world.players.get(&id) else {
        return;
    };
    let a = &world.accounts[&player.key];
    let dm = world.players.values().filter(|p| p.playing).count();
    let _ = player.outgoing.try_send(format!(
        "INFO|{}|{dm}|0|{}|{}|{}",
        world.players.len(),
        a.rating,
        a.wins,
        a.losses
    ));
    let mut top: Vec<_> = world
        .accounts
        .values()
        .filter(|a| a.wins + a.losses > 0)
        .collect();
    top.sort_unstable_by(|a, b| {
        b.rating
            .cmp(&a.rating)
            .then(b.wins.cmp(&a.wins))
            .then(a.name.cmp(&b.name))
    });
    let top = top
        .into_iter()
        .take(10)
        .map(|a| format!("{}:{}:{}:{}", a.name, a.rating, a.wins, a.losses))
        .collect::<Vec<_>>()
        .join(",");
    let _ = player.outgoing.try_send(format!("TOP|{top}"));
}

fn play(world: &mut World, id: u32, rng: &mut Rng) {
    if world.players[&id].playing {
        return;
    }
    let count = world.players.values().filter(|p| p.playing).count();
    if count >= MAX_PLAYERS {
        let _ = world.players[&id]
            .outgoing
            .try_send("ERROR|Server is full".to_owned());
        return;
    }
    let boys = world
        .players
        .values()
        .filter(|p| p.playing && p.team == "boys")
        .count();
    let team = choose_team(boys, count - boys, rng);
    let (x, z, y) = team_spawn(team, rng);
    let existing: Vec<_> = world
        .players
        .iter()
        .filter(|(_, p)| p.playing)
        .map(|(&id, p)| {
            format!(
                "ADD|{id}|{}|{}|{}|{}|{}|{}|{}",
                p.name, p.x, p.z, p.y, p.yaw, p.health, p.team
            )
        })
        .collect();
    let p = world.players.get_mut(&id).unwrap();
    (p.x, p.z, p.y, p.yaw, p.health, p.team, p.playing) = (
        x,
        z,
        y,
        -std::f32::consts::FRAC_PI_2,
        MAX_HEALTH,
        team,
        true,
    );
    let _ = p
        .outgoing
        .try_send(format!("WELCOME|{id}|{x}|{z}|{y}|{MAX_HEALTH}|{team}"));
    let _ = p
        .outgoing
        .try_send(format!("SCORE|{}|{}", world.boys, world.girls));
    if let Some(winner) = world.winner {
        let remaining = world
            .reset_at
            .unwrap()
            .saturating_duration_since(Instant::now())
            .as_secs()
            + 1;
        let _ = p.outgoing.try_send(format!("WIN|{winner}|{remaining}"));
    }
    for text in existing {
        let _ = p.outgoing.try_send(text);
    }
    let add = format!(
        "ADD|{id}|{}|{x}|{z}|{y}|-1.5707964|{MAX_HEALTH}|{team}",
        p.name
    );
    broadcast(&mut world.players, id, &add);
}

type Shared = Arc<Mutex<World>>;

// xorshift64*; per connection, seeded from the clock and id for spawn jitter.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

// A random point on the team's spawn terrace, snapped to its floor.
fn team_spawn(team: &str, rng: &mut Rng) -> (f32, f32, f32) {
    let (area, floor) = if team == "girls" {
        (map::CAMEL_CT_SPAWN, map::CAMEL_CT_FLOOR)
    } else {
        (map::CAMEL_T_SPAWN, map::CAMEL_T_FLOOR)
    };
    for _ in 0..40 {
        let x = area[0] + rng.next_f32() * (area[2] - area[0]);
        let z = area[1] + rng.next_f32() * (area[3] - area[1]);
        if let Some(y) = map::stand_height(map::Map::Camel, x, z, floor, 0.28) {
            return (x, z, y);
        }
    }
    ((area[0] + area[2]) * 0.5, (area[1] + area[3]) * 0.5, floor)
}

fn choose_team(boys: usize, girls: usize, rng: &mut Rng) -> &'static str {
    if boys < girls || (boys == girls && rng.next_u64() & 1 == 0) {
        "boys"
    } else {
        "girls"
    }
}

// A client whose queue is full has fallen behind; dropping it beats letting it
// miss state such as HP, SCORE or LEAVE.
fn broadcast(players: &mut HashMap<u32, Player>, except: u32, message: &str) {
    players.retain(|&id, player| {
        !player.playing || id == except || player.outgoing.try_send(message.to_owned()).is_ok()
    });
}

// Called from every connection loop; the first thread to see the deadline
// resets the round for everyone.
fn maybe_reset(world: &mut World, rng: &mut Rng) {
    let Some(at) = world.reset_at else {
        return;
    };
    if Instant::now() < at {
        return;
    }
    world.boys = 0;
    world.girls = 0;
    world.winner = None;
    world.reset_at = None;
    let mut moves = Vec::new();
    for (&id, player) in world.players.iter_mut() {
        if !player.playing {
            continue;
        }
        let (x, z, y) = team_spawn(player.team, rng);
        player.x = x;
        player.z = z;
        player.y = y;
        player.yaw = -std::f32::consts::FRAC_PI_2;
        player.health = MAX_HEALTH;
        let _ = player.outgoing.try_send(format!("AT|{x}|{y}|{z}"));
        moves.push((id, x, z, y));
    }
    for (id, x, z, y) in moves {
        broadcast(&mut world.players, 0, &format!("HP|{id}|{MAX_HEALTH}"));
        broadcast(
            &mut world.players,
            id,
            &format!("MOVE|{id}|{x}|{z}|{y}|-1.5707964"),
        );
    }
    broadcast(&mut world.players, 0, "SCORE|0|0");
}

// Adapted from aevyrie/bevy_mod_raycast/src/primitives.rs, intersects_aabb,
// lines 164-203: https://github.com/aevyrie/bevy_mod_raycast/blob/main/src/primitives.rs
// License: MIT (see LICENSE-MIT-bevy_mod_raycast). World-space slab test, with
// a parallel-axis check so a ray beside a box cannot hit it.
fn ray_box(origin: [f32; 3], direction: [f32; 3], min: [f32; 3], max: [f32; 3]) -> Option<f32> {
    let (mut near, mut far) = (0.0_f32, 100.0_f32);
    for axis in 0..3 {
        if direction[axis].abs() < 0.000001 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
        } else {
            let a = (min[axis] - origin[axis]) / direction[axis];
            let b = (max[axis] - origin[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    Some(near)
}

fn handle_client(stream: TcpStream, shared: Shared, ids: Arc<AtomicU32>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let config = WebSocketConfig::default()
        .read_buffer_size(4096)
        .write_buffer_size(4096)
        .max_write_buffer_size(8192)
        .max_message_size(Some(256))
        .max_frame_size(Some(256));
    let Ok(peer) = stream.peer_addr().map(|address| address.ip()) else {
        return;
    };
    let mut forwarded = None;
    #[allow(clippy::result_large_err)] // tungstenite fixes the callback signature.
    let read_forwarded = |request: &tungstenite::handshake::server::Request, response| {
        forwarded = request.headers().get("x-forwarded-for").cloned();
        Ok(response)
    };
    let Ok(mut socket) = accept_hdr_with_config(stream, read_forwarded, Some(config)) else {
        return;
    };
    // Behind a local proxy such as Caddy in Docker, every peer is the proxy, so
    // use the address it appends. Direct clients could forge the header.
    let proxied = peer.is_loopback() || matches!(peer, IpAddr::V4(ip) if ip.is_private());
    let ip = forwarded
        .filter(|_| proxied)
        .and_then(|value| value.to_str().ok()?.rsplit(',').next()?.trim().parse().ok())
        .unwrap_or(peer);
    let Ok(Message::Text(auth)) = socket.read() else {
        return;
    };
    let id = ids.fetch_add(1, Ordering::Relaxed);
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (id as u64) << 32;
    let mut rng = Rng::new(seed);
    let (outgoing, incoming) = mpsc::sync_channel::<String>(128);
    let response = match authenticate(&shared, ip, &auth, id, outgoing) {
        Ok(response) => response,
        Err(code) => {
            let _ = socket.send(Message::Text(format!("AUTH_ERR|{code}").into()));
            return;
        }
    };
    let ready = socket.send(Message::Text(response.into())).is_ok();
    let _ = socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(50)));

    if ready {
        let mut last_fire = Instant::now() - Duration::from_secs(1);
        let mut last_pos = last_fire;
        let mut last_info = last_fire;
        let (mut last_heard, mut last_ping) = (Instant::now(), Instant::now());
        'connected: loop {
            // Browsers answer pings on their own, even while paused, so silence
            // means a dead link that would otherwise keep the slot and nickname.
            if last_heard.elapsed() > Duration::from_secs(15) {
                break;
            }
            if last_ping.elapsed() > Duration::from_secs(5) {
                last_ping = Instant::now();
                if socket.send(Message::Ping(Default::default())).is_err() {
                    break;
                }
            }
            while let Ok(message) = incoming.try_recv() {
                if socket.send(Message::Text(message.into())).is_err() {
                    break 'connected;
                }
            }
            let read = socket.read();
            if read.is_ok() {
                last_heard = Instant::now();
            }
            match read {
                Ok(Message::Text(text)) => {
                    {
                        let mut world = shared.lock().unwrap();
                        if !world.players.contains_key(&id) {
                            break;
                        }
                        match text.as_str() {
                            "LOGOUT" => {
                                let key = world.players[&id].key.clone();
                                world.sessions.retain(|_, owner| owner != &key);
                                break;
                            }
                            "INFO" => {
                                if last_info.elapsed() >= Duration::from_secs(1) {
                                    info(&world, id);
                                    last_info = Instant::now();
                                }
                                continue;
                            }
                            "PLAY" => {
                                play(&mut world, id, &mut rng);
                                continue;
                            }
                            "LOBBY" => {
                                if world.players[&id].playing {
                                    world.players.get_mut(&id).unwrap().playing = false;
                                    broadcast(&mut world.players, id, &format!("LEAVE|{id}"));
                                }
                                info(&world, id);
                                continue;
                            }
                            _ => {}
                        }
                        if !world.players[&id].playing {
                            continue;
                        }
                    }
                    let mut fields = text.split('|');
                    match fields.next() {
                        Some("POS") => {
                            let (Some(x), Some(z), Some(feet), Some(yaw), None) = (
                                fields.next(),
                                fields.next(),
                                fields.next(),
                                fields.next(),
                                fields.next(),
                            ) else {
                                continue;
                            };
                            let (Ok(x), Ok(z), Ok(feet), Ok(yaw)) = (
                                x.parse::<f32>(),
                                z.parse::<f32>(),
                                feet.parse::<f32>(),
                                yaw.parse::<f32>(),
                            ) else {
                                continue;
                            };
                            // Clients send 32 times a second; a faster sender would flood
                            // everyone else's queue.
                            if ![x, z, feet, yaw].iter().all(|v| v.is_finite())
                                || last_pos.elapsed() < Duration::from_millis(15)
                            {
                                continue;
                            }
                            // Checked from the client's own feet, so walking off a
                            // ledge or jumping onto a crate never leaves the server
                            // stuck at an old height.
                            // Radius 0: a client stepping out of a wall it landed in is
                            // still partly inside it. ponytail: walls are not checked
                            // here; trace the move segment if wall cheats matter.
                            let Some(floor) = map::stand_height(map::Map::Camel, x, z, feet, 0.0)
                            else {
                                continue;
                            };
                            // Up to a jump above the floor, so others see jumps and
                            // the hitbox follows them.
                            let y = feet.clamp(floor, floor + 1.0);
                            let mut world = shared.lock().unwrap();
                            let Some(player) = world.players.get_mut(&id) else {
                                continue;
                            };
                            // Walking speed is 4 m/s; the extra metre covers jitter.
                            // ponytail: standing still banks travel time; send position
                            // corrections if that is ever abused.
                            let reach = 4.0 * last_pos.elapsed().as_secs_f32() + 1.0;
                            if player.health == 0 || (x - player.x).hypot(z - player.z) > reach {
                                continue;
                            }
                            last_pos = Instant::now();
                            player.x = x;
                            player.z = z;
                            player.y = y;
                            player.yaw = yaw;
                            broadcast(
                                &mut world.players,
                                id,
                                &format!("MOVE|{id}|{x}|{z}|{y}|{yaw}"),
                            );
                        }
                        Some("FIRE") => {
                            let (Some(yaw), Some(pitch), None) =
                                (fields.next(), fields.next(), fields.next())
                            else {
                                continue;
                            };
                            let (Ok(yaw), Ok(pitch)) = (yaw.parse::<f32>(), pitch.parse::<f32>())
                            else {
                                continue;
                            };
                            // The client waits 500 ms; the margin absorbs network jitter.
                            if !yaw.is_finite()
                                || !pitch.is_finite()
                                || pitch.abs() > 1.5
                                || last_fire.elapsed() < Duration::from_millis(450)
                            {
                                continue;
                            }
                            let mut world = shared.lock().unwrap();
                            if world.winner.is_some() {
                                continue;
                            }
                            let Some((sx, sz, sy, health, team)) = world
                                .players
                                .get(&id)
                                .map(|p| (p.x, p.z, p.y, p.health, p.team))
                            else {
                                continue;
                            };
                            if health == 0 {
                                continue;
                            }
                            last_fire = Instant::now();
                            let origin = [sx, sy + 1.7, sz];
                            let direction = [
                                yaw.cos() * pitch.cos(),
                                pitch.sin(),
                                yaw.sin() * pitch.cos(),
                            ];
                            let mut nearest = map::ray_map(origin, direction);
                            let mut hit = None;
                            for (&target_id, target) in world.players.iter() {
                                if target_id == id
                                    || !target.playing
                                    || target.health == 0
                                    || target.team == team
                                {
                                    continue;
                                }
                                if let Some(distance) = ray_box(
                                    origin,
                                    direction,
                                    [target.x - 0.35, target.y, target.z - 0.35],
                                    [target.x + 0.35, target.y + 1.9, target.z + 0.35],
                                ) {
                                    if distance < nearest {
                                        nearest = distance;
                                        hit = Some(target_id);
                                    }
                                }
                            }
                            if let Some(target_id) = hit {
                                let target_health = {
                                    let target = world.players.get_mut(&target_id).unwrap();
                                    target.health -= 1;
                                    target.health
                                };
                                broadcast(
                                    &mut world.players,
                                    0,
                                    &format!("HP|{target_id}|{target_health}"),
                                );
                                if target_health == 0 {
                                    if team == "girls" {
                                        world.girls += 1;
                                    } else {
                                        world.boys += 1;
                                    }
                                    let (boys, girls) = (world.boys, world.girls);
                                    broadcast(
                                        &mut world.players,
                                        0,
                                        &format!("SCORE|{boys}|{girls}"),
                                    );
                                    if boys >= map::WIN_SCORE || girls >= map::WIN_SCORE {
                                        let winner = if boys >= map::WIN_SCORE {
                                            "boys"
                                        } else {
                                            "girls"
                                        };
                                        world.winner = Some(winner);
                                        world.reset_at = Some(Instant::now() + ROUND_RESET);
                                        broadcast(
                                            &mut world.players,
                                            0,
                                            &format!("WIN|{winner}|{}", ROUND_RESET.as_secs()),
                                        );
                                    }
                                }
                            }
                        }
                        Some("RESPAWN") => {
                            let mut world = shared.lock().unwrap();
                            if world.winner.is_some() {
                                continue;
                            }
                            let Some(player) = world.players.get(&id) else {
                                continue;
                            };
                            if player.health != 0 {
                                continue;
                            }
                            let team = player.team;
                            let (x, z, y) = team_spawn(team, &mut rng);
                            {
                                let player = world.players.get_mut(&id).unwrap();
                                player.x = x;
                                player.z = z;
                                player.y = y;
                                player.yaw = -std::f32::consts::FRAC_PI_2;
                                player.health = MAX_HEALTH;
                                let _ = player.outgoing.try_send(format!("AT|{x}|{y}|{z}"));
                            }
                            broadcast(&mut world.players, 0, &format!("HP|{id}|{MAX_HEALTH}"));
                            broadcast(
                                &mut world.players,
                                id,
                                &format!("MOVE|{id}|{x}|{z}|{y}|-1.5707964"),
                            );
                        }
                        _ => {}
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(Error::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => break,
                _ => {}
            }
            let mut world = shared.lock().unwrap();
            if !world.players.contains_key(&id) {
                break;
            }
            maybe_reset(&mut world, &mut rng);
        }
    }
    let mut world = shared.lock().unwrap();
    if world.players.remove(&id).is_some_and(|p| p.playing) {
        broadcast(&mut world.players, id, &format!("LEAVE|{id}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "keles-{}-{}.tsv",
            std::process::id(),
            accounts::token()
        ))
    }

    #[test]
    fn accounts_authentication_limits_and_file_roundtrip() {
        let path = test_path();
        let shared = Arc::new(Mutex::new(World::new(path.clone()).unwrap()));
        let ip = "127.0.0.1".parse().unwrap();
        let (tx, _rx) = mpsc::sync_channel(128);
        let auth = |id, text: &str| authenticate(&shared, ip, text, id, tx.clone());
        let ok = auth(1, "REGISTER|UserA|password|with|pipes").unwrap();
        assert!(ok.starts_with("AUTH_OK|UserA|"));
        assert_eq!(auth(2, "REGISTER|UserA|password1"), Err("taken"));
        assert_eq!(auth(2, "REGISTER|usera|password1"), Err("taken"));
        assert_eq!(auth(2, "LOGIN|UserA|wrongpass"), Err("wrong"));
        assert_eq!(auth(2, "LOGIN|Missing|wrongpass"), Err("wrong"));
        assert_eq!(auth(2, "LOGIN|usera|password|with|pipes"), Err("online"));
        let token = ok.split('|').nth(2).unwrap();
        shared.lock().unwrap().players.remove(&1);
        assert!(auth(2, &format!("TOKEN|{token}")).is_ok());
        {
            let mut world = shared.lock().unwrap();
            world.players.remove(&2);
            world.sessions.retain(|_, key| key != "usera");
            assert_eq!(accounts::load(&path).unwrap(), world.accounts);
        }
        assert_eq!(auth(3, &format!("TOKEN|{token}")), Err("expired"));
        assert!(auth(3, "LOGIN|USERA|password|with|pipes").is_ok());
        assert_eq!(auth(4, "LOGIN|Missing|wrongpass"), Err("wrong"));
        assert_eq!(auth(4, "LOGIN|Missing|wrongpass"), Err("rate_limited"));
        assert_eq!(accounts::elo(1000, 1000, 1.0), 1016);
        assert_eq!(accounts::elo(1000, 1000, 0.0), 984);
        assert_eq!(accounts::elo(1000, 1000, 0.5), 1000);
        assert!(!accounts::valid_username("ab"));
        assert!(!accounts::valid_password("short"));
        assert!(!accounts::valid_password("password\n"));
        std::fs::write(&path, "# keles accounts v1\nbroken").unwrap();
        assert!(accounts::load(&path).is_err());
        std::fs::remove_file(path).unwrap();
        let mut world = shared.lock().unwrap();
        world.auth_attempts.clear();
        world.registrations.clear();
        for _ in 0..3 {
            assert!(!limited(&mut world.registrations, ip, 3600, 3));
        }
        assert!(limited(&mut world.registrations, ip, 3600, 3));
    }

    #[test]
    fn teams_balance_and_ties_can_choose_either_side() {
        let mut rng = Rng::new(7);
        assert_eq!(choose_team(1, 2, &mut rng), "boys");
        assert_eq!(choose_team(2, 1, &mut rng), "girls");
        let choices: Vec<_> = (0..32).map(|_| choose_team(2, 2, &mut rng)).collect();
        assert!(choices.contains(&"boys") && choices.contains(&"girls"));
    }

    #[test]
    fn websocket_join_movement_damage_and_rejections() {
        let shared = Arc::new(Mutex::new(World::new(test_path()).unwrap()));
        let ids = Arc::new(AtomicU32::new(1));
        let connect = || {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let shared = shared.clone();
            let ids = ids.clone();
            let handle = thread::spawn(move || {
                handle_client(listener.accept().unwrap().0, shared, ids);
            });
            let stream = TcpStream::connect(address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let (socket, _) = tungstenite::client(format!("ws://{address}/keles"), stream).unwrap();
            (socket, handle)
        };
        let read = |socket: &mut tungstenite::WebSocket<TcpStream>, prefix: &str| -> String {
            loop {
                if let Message::Text(text) = socket.read().unwrap() {
                    if text.starts_with(prefix) {
                        return text.to_string();
                    }
                }
            }
        };
        let (mut first, first_thread) = connect();
        first
            .send(Message::Text("REGISTER|First|password1".into()))
            .unwrap();
        read(&mut first, "AUTH_OK|");
        first.send(Message::Text("PLAY".into())).unwrap();
        let welcome = read(&mut first, "WELCOME|");
        let first_id: u32 = welcome.split('|').nth(1).unwrap().parse().unwrap();
        let (mut second, second_thread) = connect();
        second
            .send(Message::Text("REGISTER|Second|password2".into()))
            .unwrap();
        read(&mut second, "AUTH_OK|");
        second.send(Message::Text("PLAY".into())).unwrap();
        let other = read(&mut second, "WELCOME|");
        let second_id: u32 = other.split('|').nth(1).unwrap().parse().unwrap();
        assert_ne!(welcome.split('|').last(), other.split('|').last());
        assert!(read(&mut first, "ADD|").contains("|Second|"));
        assert!(read(&mut second, "ADD|").contains("|First|"));

        let (mut duplicate, duplicate_thread) = connect();
        duplicate
            .send(Message::Text("LOGIN|First|password1".into()))
            .unwrap();
        assert_eq!(read(&mut duplicate, "AUTH_ERR|"), "AUTH_ERR|online");
        duplicate_thread.join().unwrap();
        let (mut invalid, invalid_thread) = connect();
        invalid
            .send(Message::Text("LOGIN|bad name|password1".into()))
            .unwrap();
        assert_eq!(read(&mut invalid, "AUTH_ERR|"), "AUTH_ERR|bad_username");
        invalid_thread.join().unwrap();

        // Put the two real connections next to each other on an open spawn terrace.
        let (x, z, y) = team_spawn("boys", &mut Rng::new(7));
        assert!(map::ray_map([x, y + 1.7, z], [1.0, 0.0, 0.0]) > 0.8);
        {
            let mut world = shared.lock().unwrap();
            for (id, offset) in [(first_id, 0.0), (second_id, 0.8)] {
                let player = world.players.get_mut(&id).unwrap();
                (player.x, player.z, player.y) = (x + offset, z, y);
            }
        }
        first
            .send(Message::Text(format!("POS|{x}|{z}|{y}|0").into()))
            .unwrap();
        assert!(read(&mut second, "MOVE|").starts_with(&format!("MOVE|{first_id}|")));
        first.send(Message::Text("FIRE|0|0".into())).unwrap();
        assert_eq!(read(&mut second, "HP|"), format!("HP|{second_id}|2"));
        first.close(None).unwrap();
        second.close(None).unwrap();
        drop((first, second));
        first_thread.join().unwrap();
        second_thread.join().unwrap();

        {
            let mut world = shared.lock().unwrap();
            let (outgoing, _incoming) = mpsc::sync_channel(1);
            for id in 1..=MAX_PLAYERS as u32 {
                world.players.insert(
                    id,
                    Player {
                        name: format!("Player{id}"),
                        key: format!("player{id}"),
                        playing: true,
                        x,
                        z,
                        y,
                        yaw: 0.0,
                        health: 3,
                        team: "boys",
                        ip: IpAddr::from([127, 0, 0, 1]),
                        outgoing: outgoing.clone(),
                    },
                );
            }
        }
        let (mut full, full_thread) = connect();
        full.send(Message::Text("LOGIN|First|password1".into()))
            .unwrap();
        assert_eq!(read(&mut full, "AUTH_ERR|"), "AUTH_ERR|full");
        full_thread.join().unwrap();
    }

    #[test]
    fn reset_clears_scores_and_respawns_everyone() {
        let player = |team, outgoing| Player {
            name: String::new(),
            key: String::new(),
            playing: true,
            x: 0.0,
            z: 0.0,
            y: 0.0,
            yaw: 0.0,
            health: 0,
            team,
            ip: IpAddr::from([127, 0, 0, 1]),
            outgoing,
        };
        let (sender, receiver) = mpsc::sync_channel(32);
        let (other_sender, other_receiver) = mpsc::sync_channel(32);
        let (full_sender, _full_receiver) = mpsc::sync_channel(0);
        let mut world = World {
            players: HashMap::from([
                (1, player("girls", sender)),
                (2, player("boys", other_sender)),
                (3, player("boys", full_sender)),
            ]),
            boys: 39,
            girls: 40,
            winner: Some("girls"),
            reset_at: Some(Instant::now() - Duration::from_secs(1)),
            ..World::new(test_path()).unwrap()
        };
        let mut rng = Rng::new(7);
        maybe_reset(&mut world, &mut rng);
        assert_eq!((world.boys, world.girls), (0, 0));
        assert!(world.winner.is_none() && world.reset_at.is_none());
        assert_eq!(world.players[&1].health, MAX_HEALTH);
        assert!(
            !world.players.contains_key(&3),
            "a full queue drops the client"
        );
        let messages = receiver.try_iter().collect::<Vec<_>>();
        assert!(messages.iter().any(|m| m.starts_with("AT|")));
        assert!(messages.iter().any(|m| m == "SCORE|0|0"));
        // Other players must see the revived player again.
        assert!(other_receiver.try_iter().any(|m| m == "HP|1|3"));
    }
}

// Listener/thread pattern adapted from snapview/tungstenite-rs/README.md,
// lines 5-26: https://github.com/snapview/tungstenite-rs/blob/master/README.md
// License: MIT OR Apache-2.0 (see LICENSE-MIT-tungstenite).
fn main() -> std::io::Result<()> {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:9001".to_owned());
    let listener = TcpListener::bind(&address)?;
    println!("Keles server listening on {address}");
    let data_path = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "keles-accounts.tsv".to_owned());
    let world = World::new(data_path.into())?;
    println!("{} hesap yüklendi", world.accounts.len());
    let shared: Shared = Arc::new(Mutex::new(world));
    let ids = Arc::new(AtomicU32::new(1));
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        if active.fetch_add(1, Ordering::Relaxed) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::Relaxed);
            continue;
        }
        let shared = shared.clone();
        let ids = ids.clone();
        let active_thread = active.clone();
        if let Err(error) = thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(move || {
                handle_client(stream, shared, ids);
                active_thread.fetch_sub(1, Ordering::Relaxed);
            })
        {
            active.fetch_sub(1, Ordering::Relaxed);
            eprintln!("Could not start client thread: {error}");
        }
    }
    Ok(())
}
