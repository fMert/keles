use std::{
    collections::HashMap,
    net::{IpAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungstenite::{accept_with_config, protocol::WebSocketConfig, Error, Message};
// Shared with the client; each side uses only part of it.
#[allow(dead_code)]
#[path = "../../src/map.rs"]
mod map;

const MAX_PLAYERS: usize = 16;
// ponytail: players behind one NAT share this; raise it if they do.
const MAX_PER_IP: usize = 4;
const MAX_HEALTH: u8 = 3;
const WIN_SCORE: u32 = 40;
const ROUND_RESET: Duration = Duration::from_secs(10);

struct Player {
    name: String,
    x: f32,
    z: f32,
    y: f32,
    yaw: f32,
    health: u8,
    team: &'static str,
    outgoing: SyncSender<String>,
}

struct World {
    players: HashMap<u32, Player>,
    boys: u32,
    girls: u32,
    winner: Option<&'static str>,
    reset_at: Option<Instant>,
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

// A client whose queue is full has fallen behind; dropping it beats letting it
// miss state such as HP, SCORE or LEAVE.
fn broadcast(players: &mut HashMap<u32, Player>, except: u32, message: &str) {
    players
        .retain(|&id, player| id == except || player.outgoing.try_send(message.to_owned()).is_ok());
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
    let Ok(mut socket) = accept_with_config(stream, Some(config)) else {
        return;
    };
    let Ok(Message::Text(join)) = socket.read() else {
        return;
    };
    let Some(rest) = join.strip_prefix("JOIN|") else {
        return;
    };
    let mut join_fields = rest.split('|');
    let Some(name) = join_fields.next() else {
        return;
    };
    let team = if join_fields.next() == Some("girls") {
        "girls"
    } else {
        "boys"
    };
    if name.is_empty()
        || name.len() > 16
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        let _ = socket.send(Message::Text("ERROR|Invalid nickname".into()));
        return;
    }

    let id = ids.fetch_add(1, Ordering::Relaxed);
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (id as u64) << 32;
    let mut rng = Rng::new(seed);
    let (outgoing, incoming) = mpsc::sync_channel::<String>(64);
    let (existing, x, z, y, score, winner) = {
        let mut world = shared.lock().unwrap();
        if world.players.values().any(|player| player.name == name) {
            drop(world);
            let _ = socket.send(Message::Text("ERROR|Nickname already in use".into()));
            return;
        }
        if world.players.len() >= MAX_PLAYERS {
            drop(world);
            let _ = socket.send(Message::Text("ERROR|Server is full".into()));
            return;
        }
        let (x, z, y) = team_spawn(team, &mut rng);
        let existing = world
            .players
            .iter()
            .map(|(&id, player)| {
                format!(
                    "ADD|{id}|{}|{}|{}|{}|{}|{}|{}",
                    player.name,
                    player.x,
                    player.z,
                    player.y,
                    player.yaw,
                    player.health,
                    player.team
                )
            })
            .collect::<Vec<_>>();
        world.players.insert(
            id,
            Player {
                name: name.to_owned(),
                x,
                z,
                y,
                yaw: -std::f32::consts::FRAC_PI_2,
                health: MAX_HEALTH,
                team,
                outgoing,
            },
        );
        broadcast(
            &mut world.players,
            id,
            &format!("ADD|{id}|{name}|{x}|{z}|{y}|-1.5707964|{MAX_HEALTH}|{team}"),
        );
        let remaining = world
            .reset_at
            .map(|at| at.saturating_duration_since(Instant::now()).as_secs() + 1);
        (
            existing,
            x,
            z,
            y,
            (world.boys, world.girls),
            world.winner.map(|w| (w, remaining.unwrap_or(0))),
        )
    };
    let mut ready = socket
        .send(Message::Text(
            format!("WELCOME|{id}|{x}|{z}|{y}|{MAX_HEALTH}").into(),
        ))
        .is_ok();
    if ready {
        let _ = socket.send(Message::Text(
            format!("SCORE|{}|{}", score.0, score.1).into(),
        ));
        if let Some((winner, remaining)) = winner {
            let _ = socket.send(Message::Text(format!("WIN|{winner}|{remaining}").into()));
        }
        for player in existing {
            if socket.send(Message::Text(player.into())).is_err() {
                ready = false;
                break;
            }
        }
    }
    let _ = socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(50)));

    if ready {
        let mut last_fire = Instant::now() - Duration::from_secs(1);
        let mut last_pos = last_fire;
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
                            // Clients send every 50 ms; a faster sender would flood
                            // everyone else's queue.
                            if ![x, z, feet, yaw].iter().all(|v| v.is_finite())
                                || last_pos.elapsed() < Duration::from_millis(25)
                            {
                                continue;
                            }
                            // Checked from the client's own feet, so walking off a
                            // ledge or jumping onto a crate never leaves the server
                            // stuck at an old height.
                            let Some(floor) = map::stand_height(map::Map::Camel, x, z, feet, 0.28)
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
                                if target_id == id || target.health == 0 || target.team == team {
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
                                    if boys >= WIN_SCORE || girls >= WIN_SCORE {
                                        let winner =
                                            if boys >= WIN_SCORE { "boys" } else { "girls" };
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
    world.players.remove(&id);
    broadcast(&mut world.players, id, &format!("LEAVE|{id}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_clears_scores_and_respawns_everyone() {
        let player = |team, outgoing| Player {
            name: String::new(),
            x: 0.0,
            z: 0.0,
            y: 0.0,
            yaw: 0.0,
            health: 0,
            team,
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

fn release(open: &Mutex<HashMap<IpAddr, usize>>, ip: IpAddr) {
    let mut counts = open.lock().unwrap();
    *counts.get_mut(&ip).unwrap() -= 1;
    counts.retain(|_, count| *count > 0);
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
    let shared: Shared = Arc::new(Mutex::new(World {
        players: HashMap::new(),
        boys: 0,
        girls: 0,
        winner: None,
        reset_at: None,
    }));
    let ids = Arc::new(AtomicU32::new(1));
    let open: Arc<Mutex<HashMap<IpAddr, usize>>> = Arc::default();
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        let Ok(peer) = stream.peer_addr() else {
            continue;
        };
        let ip = peer.ip();
        {
            let mut counts = open.lock().unwrap();
            let mine = counts.get(&ip).copied().unwrap_or(0);
            // A local TLS proxy forwards everyone from loopback, so it is not capped.
            if counts.values().sum::<usize>() >= MAX_PLAYERS
                || (!ip.is_loopback() && mine >= MAX_PER_IP)
            {
                continue;
            }
            counts.insert(ip, mine + 1);
        }
        let shared = shared.clone();
        let ids = ids.clone();
        let thread_open = open.clone();
        if let Err(error) = thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(move || {
                handle_client(stream, shared, ids);
                release(&thread_open, ip);
            })
        {
            release(&open, ip);
            eprintln!("Could not start client thread: {error}");
        }
    }
    Ok(())
}
