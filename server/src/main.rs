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
const RANKED_KILLS: u32 = 10;
const RANKED_SECONDS: u64 = 300;
const COUNTDOWN: u64 = 3;

struct Player {
    key: String,
    place: Place,
    kills: u32,
    deaths: u32,
    died_at: Option<Instant>,
    failed: bool,
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

#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    Lobby,
    Queue(Instant),
    Deathmatch,
    Ranked(u32),
}

#[derive(Default)]
struct Room {
    boys: u32,
    girls: u32,
    winner: Option<&'static str>,
    reset_at: Option<Instant>,
}

struct Ranked {
    room: Room,
    players: [u32; 2],
    keys: [String; 2],
    go_at: Instant,
    ends_at: Instant,
    started: bool,
    pending: Option<(Option<u32>, &'static str)>,
    retry_at: Instant,
}

struct World {
    players: HashMap<u32, Player>,
    deathmatch: Room,
    matches: HashMap<u32, Ranked>,
    next_match: u32,
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
            deathmatch: Room::default(),
            matches: HashMap::new(),
            next_match: 1,
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
            place: Place::Lobby,
            kills: 0,
            deaths: 0,
            died_at: None,
            failed: false,
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
    let dm = world
        .players
        .values()
        .filter(|p| p.place == Place::Deathmatch)
        .count();
    let _ = player.outgoing.try_send(format!(
        "INFO|{}|{dm}|{}|{}|{}|{}",
        world.players.len(),
        world
            .players
            .values()
            .filter(|p| matches!(p.place, Place::Queue(_)))
            .count(),
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

fn add_message(id: u32, p: &Player) -> String {
    format!(
        "ADD|{id}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        p.name, p.x, p.z, p.y, p.yaw, p.health, p.team, p.kills, p.deaths
    )
}

fn enter(world: &mut World, id: u32, place: Place, team: &'static str, rng: &mut Rng) {
    let existing: Vec<_> = world
        .players
        .iter()
        .filter(|(&other, p)| other != id && p.place == place)
        .map(|(&id, p)| add_message(id, p))
        .collect();
    let (x, z, y) = team_spawn(team, rng);
    let p = world.players.get_mut(&id).unwrap();
    (p.x, p.z, p.y, p.yaw, p.health, p.team, p.place) = (
        x,
        z,
        y,
        -std::f32::consts::FRAC_PI_2,
        MAX_HEALTH,
        team,
        place,
    );
    (p.kills, p.deaths, p.died_at) = (0, 0, None);
    let _ = p
        .outgoing
        .try_send(format!("WELCOME|{id}|{x}|{z}|{y}|{MAX_HEALTH}|{team}"));
    if place == Place::Deathmatch {
        let room = &world.deathmatch;
        let _ = p
            .outgoing
            .try_send(format!("SCORE|{}|{}", room.boys, room.girls));
        if let Some(winner) = room.winner {
            let remaining = room
                .reset_at
                .unwrap()
                .saturating_duration_since(Instant::now())
                .as_secs()
                + 1;
            let _ = p.outgoing.try_send(format!("WIN|{winner}|{remaining}"));
        }
    }
    for text in existing {
        let _ = p.outgoing.try_send(text);
    }
    let add = add_message(id, p);
    broadcast(&mut world.players, place, id, &add);
}

fn play(world: &mut World, id: u32, rng: &mut Rng) {
    if world.players[&id].place != Place::Lobby {
        return;
    }
    let count = world
        .players
        .values()
        .filter(|p| p.place == Place::Deathmatch)
        .count();
    if count >= MAX_PLAYERS {
        let _ = world.players[&id]
            .outgoing
            .try_send("ERROR|Server is full".to_owned());
        return;
    }
    let boys = world
        .players
        .values()
        .filter(|p| p.place == Place::Deathmatch && p.team == "boys")
        .count();
    enter(
        world,
        id,
        Place::Deathmatch,
        choose_team(boys, count - boys, rng),
        rng,
    );
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
fn broadcast(players: &mut HashMap<u32, Player>, place: Place, except: u32, message: &str) {
    for (&id, p) in players.iter_mut() {
        if p.place == place && id != except && p.outgoing.try_send(message.to_owned()).is_err() {
            p.failed = true;
        }
    }
}

fn active(world: &World, id: u32) -> Option<Place> {
    let p = world.players.get(&id)?;
    match p.place {
        Place::Deathmatch if world.deathmatch.winner.is_none() => Some(p.place),
        Place::Ranked(id)
            if world.matches.get(&id).is_some_and(|m| {
                m.started && m.pending.is_none() && Instant::now() < m.ends_at
            }) =>
        {
            Some(p.place)
        }
        _ => None,
    }
}

fn leave(world: &mut World, id: u32) {
    let Some(place) = world.players.get(&id).map(|p| p.place) else {
        return;
    };
    if let Place::Ranked(match_id) = place {
        if let Some(m) = world.matches.get(&match_id) {
            let winner = m.players.iter().copied().find(|&other| other != id);
            let (winner, reason) = if m.started {
                (winner, "forfeit")
            } else {
                (None, "cancelled")
            };
            finish_ranked(world, match_id, winner, reason);
        }
    } else if place == Place::Deathmatch {
        broadcast(&mut world.players, place, id, &format!("LEAVE|{id}"));
    }
    if let Some(p) = world.players.get_mut(&id) {
        p.place = Place::Lobby;
    }
}

fn finish_ranked(world: &mut World, match_id: u32, winner: Option<u32>, reason: &'static str) {
    let Some(m) = world.matches.get_mut(&match_id) else {
        return;
    };
    let (winner, reason) = *m.pending.get_or_insert((winner, reason));
    let players = m.players;
    let keys = m.keys.clone();
    let old = [
        world.accounts[&keys[0]].rating,
        world.accounts[&keys[1]].rating,
    ];
    let mut updated = world.accounts.clone();
    for i in 0..2 {
        if reason == "cancelled" {
            continue;
        }
        let score = match winner {
            Some(id) if id == players[i] => 1.0,
            Some(_) => 0.0,
            None => 0.5,
        };
        let a = updated.get_mut(&keys[i]).unwrap();
        a.rating = accounts::elo(old[i], old[1 - i], score);
        // Owner decision: count draws in both columns so draw-only accounts enter TOP.
        a.wins += u32::from(score >= 0.5);
        a.losses += u32::from(score <= 0.5);
    }
    if reason != "cancelled" {
        if let Err(error) = accounts::save(&world.data_path, &updated) {
            eprintln!("Dereceli sonuç kaydedilemedi: {error}");
            m.retry_at = Instant::now() + Duration::from_secs(5);
            return;
        }
        world.accounts = updated;
    }
    world.matches.remove(&match_id);
    for i in 0..2 {
        if let Some(p) = world.players.get_mut(&players[i]) {
            p.place = Place::Lobby;
            let a = &world.accounts[&keys[i]];
            let result = match winner {
                Some(id) if id == players[i] => "win",
                Some(_) => "loss",
                None => "draw",
            };
            let delta = a.rating as i64 - old[i] as i64;
            if p.outgoing
                .try_send(format!(
                    "RANKED_END|{result}|{reason}|{}|{delta:+}|{}|{}",
                    a.rating, a.wins, a.losses
                ))
                .is_err()
            {
                p.failed = true;
            }
        }
    }
}

fn tick(world: &mut World, rng: &mut Rng) {
    let now = Instant::now();
    if world.deathmatch.reset_at.is_some_and(|at| now >= at) {
        world.deathmatch = Room::default();
        let ids: Vec<_> = world
            .players
            .iter()
            .filter(|(_, p)| p.place == Place::Deathmatch)
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            let p = world.players.get_mut(&id).unwrap();
            (p.kills, p.deaths) = (0, 0);
            revive(world, id, rng);
        }
        broadcast(&mut world.players, Place::Deathmatch, 0, "SCORE|0|0");
    }
    let failed: Vec<_> = world
        .players
        .iter()
        .filter(|(_, p)| p.failed)
        .map(|(&id, _)| id)
        .collect();
    for id in failed {
        leave(world, id);
        world.players.remove(&id);
    }
    let matches: Vec<_> = world.matches.keys().copied().collect();
    for id in matches {
        let m = world.matches.get_mut(&id).unwrap();
        if let Some((winner, reason)) = m.pending {
            if now >= m.retry_at {
                finish_ranked(world, id, winner, reason);
            }
        } else if !m.started && now >= m.go_at {
            m.started = true;
            broadcast(&mut world.players, Place::Ranked(id), 0, "GO");
        } else if m.started && now >= m.ends_at {
            let winner = if m.room.boys == m.room.girls {
                None
            } else {
                let team = if m.room.boys > m.room.girls {
                    "boys"
                } else {
                    "girls"
                };
                m.players
                    .iter()
                    .copied()
                    .find(|id| world.players.get(id).is_some_and(|p| p.team == team))
            };
            finish_ranked(world, id, winner, "time");
        }
    }
    let mut queue: Vec<_> = world
        .players
        .iter()
        .filter_map(|(&id, p)| {
            if let Place::Queue(at) = p.place {
                Some((at, id))
            } else {
                None
            }
        })
        .collect();
    queue.sort_unstable();
    for &(at, id) in &queue {
        if !matches!(world.players[&id].place, Place::Queue(_)) {
            continue;
        }
        let rating = world.accounts[&world.players[&id].key].rating;
        let other = queue.iter().find_map(|&(other_at, other)| {
            let p = &world.players[&other];
            let range =
                (100 + 50 * (now.duration_since(at.min(other_at)).as_secs() / 10)).min(1000) as u32;
            (id != other
                && matches!(p.place, Place::Queue(_))
                && (p.ip != world.players[&id].ip || p.ip.is_loopback())
                && rating.abs_diff(world.accounts[&p.key].rating) <= range)
                .then_some(other)
        });
        let Some(other) = other else {
            continue;
        };
        let match_id = world.next_match;
        world.next_match += 1;
        let players = [id, other];
        let keys = players.map(|id| world.players[&id].key.clone());
        let go_at = now + Duration::from_secs(COUNTDOWN);
        world.matches.insert(
            match_id,
            Ranked {
                room: Room::default(),
                players,
                keys,
                go_at,
                ends_at: go_at + Duration::from_secs(RANKED_SECONDS),
                started: false,
                pending: None,
                retry_at: now,
            },
        );
        let first_boys = rng.next_u64() & 1 == 0;
        for i in 0..2 {
            let p = &world.players[&players[i]];
            let opponent = &world.players[&players[1 - i]];
            let _ = p.outgoing.try_send(format!(
                "MATCH|{}|{}|{}|{RANKED_KILLS}|{RANKED_SECONDS}|{COUNTDOWN}",
                opponent.name, world.accounts[&opponent.key].rating, world.accounts[&p.key].rating
            ));
        }
        for (i, &player) in players.iter().enumerate() {
            enter(
                world,
                player,
                Place::Ranked(match_id),
                if (i == 0) == first_boys {
                    "boys"
                } else {
                    "girls"
                },
                rng,
            );
        }
        broadcast(&mut world.players, Place::Ranked(match_id), 0, "SCORE|0|0");
    }
}

fn revive(world: &mut World, id: u32, rng: &mut Rng) {
    let p = world.players.get_mut(&id).unwrap();
    let place = p.place;
    let (x, z, y) = team_spawn(p.team, rng);
    (p.x, p.z, p.y, p.yaw, p.health, p.died_at) =
        (x, z, y, -std::f32::consts::FRAC_PI_2, MAX_HEALTH, None);
    if p.outgoing.try_send(format!("AT|{x}|{y}|{z}")).is_err() {
        p.failed = true;
    }
    broadcast(
        &mut world.players,
        place,
        0,
        &format!("HP|{id}|{MAX_HEALTH}"),
    );
    broadcast(
        &mut world.players,
        place,
        id,
        &format!("MOVE|{id}|{x}|{z}|{y}|-1.5707964"),
    );
}

fn respawn(world: &mut World, id: u32, rng: &mut Rng) {
    let Some(place) = active(world, id) else {
        return;
    };
    let p = &world.players[&id];
    if p.health != 0
        || (matches!(place, Place::Ranked(_))
            && p.died_at
                .is_none_or(|at| at.elapsed() < Duration::from_secs(2)))
    {
        return;
    }
    revive(world, id, rng);
}

fn fire(world: &mut World, id: u32, yaw: f32, pitch: f32) {
    let Some(place) = active(world, id) else {
        return;
    };
    let p = &world.players[&id];
    if p.health == 0 {
        return;
    }
    let team = p.team;
    let origin = [p.x, p.y + 1.7, p.z];
    let direction = [
        yaw.cos() * pitch.cos(),
        pitch.sin(),
        yaw.sin() * pitch.cos(),
    ];
    let mut nearest = map::ray_map(origin, direction);
    let mut hit = None;
    for (&target_id, target) in &world.players {
        if target_id == id || target.place != place || target.health == 0 || target.team == team {
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
    let Some(target_id) = hit else {
        return;
    };
    let _ = world.players[&id].outgoing.try_send("HITMARK".to_owned());
    let target = world.players.get_mut(&target_id).unwrap();
    target.health -= 1;
    let health = target.health;
    if health == 0 {
        target.deaths += 1;
        target.died_at = Some(Instant::now());
    }
    broadcast(
        &mut world.players,
        place,
        0,
        &format!("HP|{target_id}|{health}"),
    );
    if health != 0 {
        return;
    }
    world.players.get_mut(&id).unwrap().kills += 1;
    broadcast(
        &mut world.players,
        place,
        0,
        &format!("KILL|{id}|{target_id}"),
    );
    let room = match place {
        Place::Deathmatch => &mut world.deathmatch,
        Place::Ranked(id) => &mut world.matches.get_mut(&id).unwrap().room,
        _ => return,
    };
    if team == "girls" {
        room.girls += 1;
    } else {
        room.boys += 1;
    }
    let (boys, girls) = (room.boys, room.girls);
    broadcast(
        &mut world.players,
        place,
        0,
        &format!("SCORE|{boys}|{girls}"),
    );
    match place {
        Place::Deathmatch if boys >= map::WIN_SCORE || girls >= map::WIN_SCORE => {
            let winner = if boys >= map::WIN_SCORE {
                "boys"
            } else {
                "girls"
            };
            world.deathmatch.winner = Some(winner);
            world.deathmatch.reset_at = Some(Instant::now() + ROUND_RESET);
            broadcast(
                &mut world.players,
                place,
                0,
                &format!("WIN|{winner}|{}", ROUND_RESET.as_secs()),
            );
        }
        Place::Ranked(match_id) if boys >= RANKED_KILLS || girls >= RANKED_KILLS => {
            finish_ranked(world, match_id, Some(id), "kills")
        }
        _ => {}
    }
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
            {
                let mut world = shared.lock().unwrap();
                tick(&mut world, &mut rng);
                if !world.players.contains_key(&id) {
                    break;
                }
            }
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
                            "QUEUE" => {
                                if world.players[&id].place == Place::Lobby {
                                    let p = world.players.get_mut(&id).unwrap();
                                    p.place = Place::Queue(Instant::now());
                                    let _ = p.outgoing.try_send("QUEUED".to_owned());
                                }
                                continue;
                            }
                            "UNQUEUE" => {
                                if matches!(world.players[&id].place, Place::Queue(_)) {
                                    let p = world.players.get_mut(&id).unwrap();
                                    p.place = Place::Lobby;
                                    let _ = p.outgoing.try_send("UNQUEUED".to_owned());
                                }
                                continue;
                            }
                            "LOBBY" => {
                                leave(&mut world, id);
                                info(&world, id);
                                continue;
                            }
                            _ => {}
                        }
                        if active(&world, id).is_none() {
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
                            let Some(place) = active(&world, id) else {
                                continue;
                            };
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
                                place,
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
                            if active(&world, id).is_some() {
                                last_fire = Instant::now();
                                fire(&mut world, id, yaw, pitch);
                            }
                        }
                        Some("RESPAWN") => {
                            let mut world = shared.lock().unwrap();
                            respawn(&mut world, id, &mut rng);
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
            tick(&mut world, &mut rng);
        }
    }
    let mut world = shared.lock().unwrap();
    leave(&mut world, id);
    world.players.remove(&id);
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

    fn ranked_world() -> (World, [mpsc::Receiver<String>; 3]) {
        let mut world = World::new(test_path()).unwrap();
        let hash = accounts::hash("password1").unwrap();
        let receivers = std::array::from_fn(|i| {
            let id = i as u32 + 1;
            let name = format!("Player{id}");
            let key = name.to_ascii_lowercase();
            world.accounts.insert(
                key.clone(),
                Account {
                    name: name.clone(),
                    hash: hash.clone(),
                    rating: 1000,
                    wins: 0,
                    losses: 0,
                },
            );
            let (outgoing, receiver) = mpsc::sync_channel(128);
            world.players.insert(
                id,
                Player {
                    key,
                    name,
                    place: if id < 3 {
                        Place::Queue(Instant::now())
                    } else {
                        Place::Lobby
                    },
                    kills: 0,
                    deaths: 0,
                    died_at: None,
                    failed: false,
                    x: 0.0,
                    z: 0.0,
                    y: 0.0,
                    yaw: 0.0,
                    health: 3,
                    team: "boys",
                    ip: "127.0.0.1".parse().unwrap(),
                    outgoing,
                },
            );
            receiver
        });
        (world, receivers)
    }

    #[test]
    fn ranked_pairing_countdown_kills_rating_and_isolation() {
        let (mut world, rx) = ranked_world();
        let mut rng = Rng::new(7);
        tick(&mut world, &mut rng);
        assert_eq!(world.matches.len(), 1);
        assert_ne!(world.players[&1].team, world.players[&2].team);
        for receiver in &rx[..2] {
            let messages: Vec<_> = receiver.try_iter().collect();
            assert!(messages[0].starts_with("MATCH|"));
            assert!(messages[1].starts_with("WELCOME|"));
            assert!(messages.iter().any(|m| m.starts_with("ADD|")));
            assert_eq!(messages.last().unwrap(), "SCORE|0|0");
        }
        let (x, z, y) = team_spawn("boys", &mut rng);
        for (id, offset) in [(1, 0.0), (2, 0.8)] {
            let p = world.players.get_mut(&id).unwrap();
            (p.x, p.z, p.y) = (x + offset, z, y);
        }
        fire(&mut world, 1, 0.0, 0.0);
        assert_eq!(world.players[&2].health, 3, "countdown ignores fire");
        let m = world.matches.get_mut(&1).unwrap();
        m.go_at = Instant::now() - Duration::from_secs(1);
        tick(&mut world, &mut rng);
        assert_eq!(rx[0].try_recv().unwrap(), "GO");
        assert_eq!(rx[1].try_recv().unwrap(), "GO");
        world.players.get_mut(&2).unwrap().health = 1;
        world.players.get_mut(&1).unwrap().kills = 9;
        let room = &mut world.matches.get_mut(&1).unwrap().room;
        if world.players[&1].team == "boys" {
            room.boys = 9;
        } else {
            room.girls = 9;
        }
        fire(&mut world, 1, 0.0, 0.0);
        assert!(world.matches.is_empty());
        assert_eq!(world.accounts["player1"].rating, 1016);
        assert_eq!(world.accounts["player2"].rating, 984);
        let messages: Vec<_> = rx[0].try_iter().collect();
        assert!(messages.iter().any(|m| m == "HITMARK"));
        assert!(messages.iter().any(|m| m == "KILL|1|2"));
        assert!(messages
            .iter()
            .any(|m| m == "RANKED_END|win|kills|1016|+16|1|0"));
        assert!(rx[1]
            .try_iter()
            .any(|m| m == "RANKED_END|loss|kills|984|-16|0|1"));
        assert!(rx[2].try_recv().is_err(), "lobby gets no room traffic");
        assert_eq!(accounts::load(&world.data_path).unwrap(), world.accounts);
        play(&mut world, 1, &mut rng);
        broadcast(&mut world.players, Place::Deathmatch, 0, "MOVE|1|0|0|0|0");
        assert!(
            rx[2].try_recv().is_err(),
            "lobby gets no deathmatch movement"
        );
        std::fs::remove_file(&world.data_path).unwrap();
    }

    #[test]
    fn ranked_leaving_cancels_or_forfeits_and_timeout_draws() {
        for started in [false, true] {
            let (mut world, rx) = ranked_world();
            tick(&mut world, &mut Rng::new(7));
            for r in &rx {
                r.try_iter().for_each(drop);
            }
            world.matches.get_mut(&1).unwrap().started = started;
            leave(&mut world, 1);
            world.players.remove(&1);
            let expected = if started {
                "RANKED_END|win|forfeit|1016|+16|1|0"
            } else {
                "RANKED_END|draw|cancelled|1000|+0|0|0"
            };
            assert_eq!(rx[1].try_recv().unwrap(), expected);
            assert_eq!(world.players[&2].place, Place::Lobby);
            if started {
                std::fs::remove_file(&world.data_path).unwrap();
            }
        }
        let (mut world, rx) = ranked_world();
        tick(&mut world, &mut Rng::new(7));
        for r in &rx {
            r.try_iter().for_each(drop);
        }
        let m = world.matches.get_mut(&1).unwrap();
        m.started = true;
        m.ends_at = Instant::now() - Duration::from_secs(1);
        tick(&mut world, &mut Rng::new(7));
        assert!(rx[0]
            .try_recv()
            .unwrap()
            .starts_with("RANKED_END|draw|time|1000|+0|"));
        assert_eq!(world.accounts["player1"].rating, 1000);
        std::fs::remove_file(&world.data_path).unwrap();
    }

    #[test]
    fn queue_rating_window_and_same_ip_guard() {
        let (mut world, _rx) = ranked_world();
        world.players.get_mut(&1).unwrap().ip = "192.0.2.1".parse().unwrap();
        world.players.get_mut(&2).unwrap().ip = "192.0.2.1".parse().unwrap();
        tick(&mut world, &mut Rng::new(7));
        assert!(world.matches.is_empty());
        world.players.get_mut(&2).unwrap().ip = "192.0.2.2".parse().unwrap();
        world.accounts.get_mut("player2").unwrap().rating = 1200;
        tick(&mut world, &mut Rng::new(7));
        assert!(world.matches.is_empty());
        world.players.get_mut(&1).unwrap().place =
            Place::Queue(Instant::now() - Duration::from_secs(20));
        tick(&mut world, &mut Rng::new(7));
        assert_eq!(world.matches.len(), 1);
        assert_eq!(world.matches[&1].players[0], 1, "oldest waiter first");
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
                .set_read_timeout(Some(Duration::from_secs(10)))
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

        let (mut login, login_thread) = connect();
        login
            .send(Message::Text("LOGIN|First|password1".into()))
            .unwrap();
        let auth = read(&mut login, "AUTH_OK|");
        let token = auth.split('|').nth(2).unwrap();
        login.send(Message::Text("LOGOUT".into())).unwrap();
        login_thread.join().unwrap();
        let (mut expired, expired_thread) = connect();
        expired
            .send(Message::Text(format!("TOKEN|{token}").into()))
            .unwrap();
        assert_eq!(read(&mut expired, "AUTH_ERR|"), "AUTH_ERR|expired");
        expired_thread.join().unwrap();

        {
            let mut world = shared.lock().unwrap();
            let (outgoing, _incoming) = mpsc::sync_channel(1);
            for id in 1..=MAX_PLAYERS as u32 {
                world.players.insert(
                    id,
                    Player {
                        name: format!("Player{id}"),
                        key: format!("player{id}"),
                        place: Place::Deathmatch,
                        kills: 0,
                        deaths: 0,
                        died_at: None,
                        failed: false,
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
        std::fs::remove_file(&shared.lock().unwrap().data_path).unwrap();
    }

    #[test]
    fn reset_clears_scores_and_respawns_everyone() {
        let player = |team, outgoing| Player {
            name: String::new(),
            key: String::new(),
            place: Place::Deathmatch,
            kills: 0,
            deaths: 0,
            died_at: None,
            failed: false,
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
            deathmatch: Room {
                boys: 39,
                girls: 40,
                winner: Some("girls"),
                reset_at: Some(Instant::now() - Duration::from_secs(1)),
            },
            ..World::new(test_path()).unwrap()
        };
        let mut rng = Rng::new(7);
        tick(&mut world, &mut rng);
        assert_eq!((world.deathmatch.boys, world.deathmatch.girls), (0, 0));
        assert!(world.deathmatch.winner.is_none() && world.deathmatch.reset_at.is_none());
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
