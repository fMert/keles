use std::{
    collections::HashMap,
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{accept_with_config, protocol::WebSocketConfig, Error, Message};
#[path = "../../src/map.rs"]
mod map;

struct Player {
    slot: usize,
    name: String,
    x: f32,
    z: f32,
    y: f32,
    yaw: f32,
    health: u8,
    outgoing: SyncSender<String>,
}

type Players = Arc<Mutex<HashMap<u32, Player>>>;

fn broadcast(players: &HashMap<u32, Player>, except: u32, message: &str) {
    for (&id, player) in players {
        if id != except {
            let _ = player.outgoing.try_send(message.to_owned());
        }
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

fn handle_client(stream: TcpStream, players: Players, ids: Arc<AtomicU32>) {
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
    let Some(name) = join.strip_prefix("JOIN|") else {
        return;
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
    let (outgoing, incoming) = mpsc::sync_channel::<String>(32);
    let (existing, x, z) = {
        let mut players = players.lock().unwrap();
        if players.values().any(|player| player.name == name) {
            drop(players);
            let _ = socket.send(Message::Text("ERROR|Nickname already in use".into()));
            return;
        }
        let Some(slot) = (0..16).find(|slot| players.values().all(|player| player.slot != *slot))
        else {
            drop(players);
            let _ = socket.send(Message::Text("ERROR|Server is full".into()));
            return;
        };
        let candidate_x = map::CAMEL_SPAWN[0] + (slot % 4) as f32 * 0.6;
        let candidate_z = map::CAMEL_SPAWN[2] - (slot / 4) as f32 * 0.6;
        let (x, z, y) = map::stand_height(map::Map::Camel, candidate_x, candidate_z, map::CAMEL_SPAWN[1], 0.28)
            .map(|y| (candidate_x, candidate_z, y))
            .unwrap_or((map::CAMEL_SPAWN[0], map::CAMEL_SPAWN[2], map::CAMEL_SPAWN[1]));
        let existing = players
            .iter()
            .map(|(&id, player)| {
                format!(
                    "ADD|{id}|{}|{}|{}|{}|{}",
                    player.name, player.x, player.z, player.yaw, player.health
                )
            })
            .collect::<Vec<_>>();
        players.insert(
            id,
            Player {
                slot,
                name: name.to_owned(),
                x,
                z,
                y,
                yaw: -std::f32::consts::FRAC_PI_2,
                health: 3,
                outgoing,
            },
        );
        broadcast(
            &players,
            id,
            &format!("ADD|{id}|{name}|{x}|{z}|-1.5707964|3"),
        );
        (existing, x, z)
    };
    let mut ready = socket
        .send(Message::Text(format!("WELCOME|{id}|{x}|{z}|3").into()))
        .is_ok();
    let mut last_fire = Instant::now() - Duration::from_secs(1);
    if ready {
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
        'connected: loop {
            while let Ok(message) = incoming.try_recv() {
                if socket.send(Message::Text(message.into())).is_err() {
                    break 'connected;
                }
            }
            match socket.read() {
                Ok(Message::Text(text)) => {
                    let mut fields = text.split('|');
                    match fields.next() {
                        Some("POS") => {
                            let (Some(x), Some(z), Some(yaw), None) =
                                (fields.next(), fields.next(), fields.next(), fields.next())
                            else {
                                continue;
                            };
                            let (Ok(x), Ok(z), Ok(yaw)) =
                                (x.parse::<f32>(), z.parse::<f32>(), yaw.parse::<f32>())
                            else {
                                continue;
                            };
                            if !x.is_finite()
                                || !z.is_finite()
                                || !yaw.is_finite()
                            {
                                continue;
                            }
                            let mut players = players.lock().unwrap();
                            if let Some(player) = players.get_mut(&id) {
                                if player.health == 0 {
                                    continue;
                                }
                                let Some(y) = map::stand_height(map::Map::Camel, x, z, player.y, 0.28) else {
                                    continue;
                                };
                                player.x = x;
                                player.z = z;
                                player.y = y;
                                player.yaw = yaw;
                            }
                            broadcast(&players, id, &format!("MOVE|{id}|{x}|{z}|{yaw}"));
                        }
                        Some("FIRE") => {
                            let (Some(x), Some(z), Some(yaw), Some(pitch), None) = (
                                fields.next(),
                                fields.next(),
                                fields.next(),
                                fields.next(),
                                fields.next(),
                            ) else {
                                continue;
                            };
                            let (Ok(x), Ok(z), Ok(yaw), Ok(pitch)) = (
                                x.parse::<f32>(),
                                z.parse::<f32>(),
                                yaw.parse::<f32>(),
                                pitch.parse::<f32>(),
                            ) else {
                                continue;
                            };
                            if ![x, z, yaw, pitch].iter().all(|v| v.is_finite())
                                || pitch.abs() > 1.5
                                || last_fire.elapsed() < Duration::from_millis(240)
                            {
                                continue;
                            }
                            let mut players = players.lock().unwrap();
                            let Some(shooter) = players.get(&id) else {
                                continue;
                            };
                            if (x - shooter.x).abs() > 1.0
                                || (z - shooter.z).abs() > 1.0
                                || shooter.health == 0
                            {
                                continue;
                            }
                            last_fire = Instant::now();
                            let origin = [x, shooter.y + 1.7, z];
                            let direction = [
                                yaw.cos() * pitch.cos(),
                                pitch.sin(),
                                yaw.sin() * pitch.cos(),
                            ];
                            let mut nearest = map::ray_map(origin, direction);
                            let mut hit = None;
                            for (&target_id, target) in players.iter() {
                                if target_id == id || target.health == 0 {
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
                                let target = players.get_mut(&target_id).unwrap();
                                target.health -= 1;
                                let health = target.health;
                                broadcast(&players, 0, &format!("HP|{target_id}|{health}"));
                            }
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
        }
    }
    let mut players = players.lock().unwrap();
    players.remove(&id);
    broadcast(&players, id, &format!("LEAVE|{id}"));
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
    let players: Players = Arc::new(Mutex::new(HashMap::new()));
    let ids = Arc::new(AtomicU32::new(1));
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        if active.fetch_add(1, Ordering::Relaxed) >= 16 {
            active.fetch_sub(1, Ordering::Relaxed);
            continue;
        }
        let players = players.clone();
        let ids = ids.clone();
        let active_thread = active.clone();
        if let Err(error) = thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(move || {
                handle_client(stream, players, ids);
                active_thread.fetch_sub(1, Ordering::Relaxed);
            })
        {
            active.fetch_sub(1, Ordering::Relaxed);
            eprintln!("Could not start client thread: {error}");
        }
    }
    Ok(())
}
