// Camel's mesh and textures come from the user's Sketchfab GLB (CC BY 4.0):
// https://sketchfab.com/3d-models/de-dust2-cs-map-056008d59eb849a29c0ab6884c0c3d87
use std::sync::OnceLock;
#[derive(Clone, Copy, PartialEq)]
pub enum Map {
    Test,
    Camel,
}

// The Camel mesh is natively small relative to the 1.9 m player, so the world
// is scaled up to match. Tune here; both rendering and collision use it.
pub const CAMEL_SCALE: f32 = 1.4;

pub const CAMEL_SPAWN: [f32; 3] = [-20.0 * CAMEL_SCALE, 3.4 * CAMEL_SCALE, 20.0 * CAMEL_SCALE];

// Team spawn areas on the de_dust2 replica, as world-space [x0, z0, x1, z1]
// rectangles, plus each terrace's floor height. de_dust2's
// info_player_terrorist / info_player_counterterrorist entities sit at Source
// coordinates; the mesh keeps Source units before the GLB root transform
// (scale 1/75, model-up = Source Z) and the converter's recentering, so a
// Source point maps to world x = (X/75 + 4.2667) * CAMEL_SCALE and
// z = (-Y/75 + 14.9333) * CAMEL_SCALE, with the floor following Source Z. T
// spawn is the raised southern terrace; CT spawn is the lower underpass.
pub const CAMEL_T_SPAWN: [f32; 4] = [-16.24, 34.53, -6.16, 36.77];
pub const CAMEL_T_FLOOR: f32 = 5.97;
pub const CAMEL_CT_SPAWN: [f32; 4] = [8.77, -25.57, 12.69, -22.87];
pub const CAMEL_CT_FLOOR: f32 = 1.19;

pub struct Block {
    pub center: [f32; 3],
    pub half: [f32; 3],
    pub color: [f32; 3],
}

pub const TEST: &[Block] = &[
    Block {
        center: [0.0, -0.05, 0.0],
        half: [20.0, 0.05, 20.0],
        color: [0.25, 0.46, 0.29],
    },
    Block {
        center: [-3.0, 1.0, -3.0],
        half: [1.0, 1.0, 1.0],
        color: [0.82, 0.40, 0.31],
    },
    Block {
        center: [2.0, 0.8, -5.0],
        half: [0.8, 0.8, 0.8],
        color: [0.91, 0.72, 0.31],
    },
    Block {
        center: [4.0, 1.4, 1.0],
        half: [0.8, 1.4, 0.8],
        color: [0.38, 0.55, 0.77],
    },
    Block {
        center: [-5.0, 0.6, 2.0],
        half: [1.2, 0.6, 0.8],
        color: [0.72, 0.57, 0.76],
    },
];

pub fn blocks(map: Map) -> &'static [Block] {
    match map {
        Map::Test => TEST,
        Map::Camel => &[],
    }
}

struct Face {
    p: [[f32; 3]; 3],
    normal_y: f32,
    min: [f32; 3],
    max: [f32; 3],
}

static FACES: OnceLock<Vec<Face>> = OnceLock::new();

fn faces() -> &'static [Face] {
    FACES.get_or_init(|| {
        let bytes = include_bytes!("../assets/camel_mesh.bin");
        let mut vertices = Vec::with_capacity(bytes.len() / 20);
        for vertex in bytes.as_chunks::<20>().0 {
            let coordinate =
                |i| f32::from_le_bytes(vertex[i..i + 4].try_into().unwrap()) * CAMEL_SCALE;
            vertices.push([coordinate(0), coordinate(4), coordinate(8)]);
        }
        vertices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|v| {
                let a = sub(v[1], v[0]);
                let b = sub(v[2], v[0]);
                let n = cross(a, b);
                let length = dot(n, n).sqrt().max(0.00001);
                let mut min = v[0];
                let mut max = v[0];
                for p in &v[1..] {
                    for axis in 0..3 {
                        min[axis] = min[axis].min(p[axis]);
                        max[axis] = max[axis].max(p[axis]);
                    }
                }
                Face {
                    p: [v[0], v[1], v[2]],
                    normal_y: n[1] / length,
                    min,
                    max,
                }
            })
            .collect()
    })
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

// Player-relative constants: the eye sits EYE_HEIGHT above the feet, and faces
// no taller than STEP_UP are climbed without a jump.
pub const EYE_HEIGHT: f32 = 1.7;
pub const STEP_UP: f32 = 0.75;

// Height where a floor/step/roof face crosses (x, z), if it does.
fn floor_y(face: &Face, x: f32, z: f32) -> Option<f32> {
    let [a, b, c] = face.p;
    let denom = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
    if denom.abs() < 0.00001 {
        return None;
    }
    let u = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / denom;
    let v = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / denom;
    if u < -0.0001 || v < -0.0001 || u + v > 1.0001 {
        return None;
    }
    Some(a[1] * u + b[1] * v + c[1] * (1.0 - u - v))
}

// Highest walkable surface at (x, z) within `rise` above the feet. None means
// there is nothing to stand on there, so the player falls.
pub fn floor_at(map: Map, x: f32, z: f32, feet: f32, rise: f32) -> Option<f32> {
    if map == Map::Test {
        if x.abs() > 20.0 || z.abs() > 20.0 {
            return None;
        }
        let mut floor: Option<f32> = (0.0 <= feet + rise).then_some(0.0);
        for block in TEST.iter().skip(1) {
            if (x - block.center[0]).abs() > block.half[0]
                || (z - block.center[2]).abs() > block.half[2]
            {
                continue;
            }
            let top = block.center[1] + block.half[1];
            if top <= feet + rise && floor.is_none_or(|f| top > f) {
                floor = Some(top);
            }
        }
        return floor;
    }
    if x.abs() > 30.0 * CAMEL_SCALE || z.abs() > 36.0 * CAMEL_SCALE {
        return None;
    }
    let mut floor: Option<f32> = None;
    for face in faces() {
        if face.normal_y.abs() < 0.65
            || x < face.min[0]
            || x > face.max[0]
            || z < face.min[2]
            || z > face.max[2]
        {
            continue;
        }
        if let Some(y) = floor_y(face, x, z) {
            if y <= feet + rise && floor.is_none_or(|f| y > f) {
                floor = Some(y);
            }
        }
    }
    floor
}

// A near-vertical face rising above the climbable step and overlapping the
// player's body blocks movement at (x, z).
pub fn wall_blocked(map: Map, x: f32, z: f32, feet: f32, radius: f32) -> bool {
    if map == Map::Test {
        return TEST.iter().skip(1).any(|block| {
            (x - block.center[0]).abs() < block.half[0] + radius
                && (z - block.center[2]).abs() < block.half[2] + radius
                && block.center[1] + block.half[1] > feet + STEP_UP
        });
    }
    for face in faces() {
        if face.normal_y.abs() > 0.6
            || face.max[1] <= feet + STEP_UP
            || face.min[1] > feet + EYE_HEIGHT
            || x + radius < face.min[0]
            || x - radius > face.max[0]
            || z + radius < face.min[2]
            || z - radius > face.max[2]
        {
            continue;
        }
        for edge in 0..3 {
            let a = face.p[edge];
            let b = face.p[(edge + 1) % 3];
            let dx = b[0] - a[0];
            let dz = b[2] - a[2];
            let t = (((x - a[0]) * dx + (z - a[2]) * dz) / (dx * dx + dz * dz).max(0.00001))
                .clamp(0.0, 1.0);
            if (x - a[0] - t * dx).powi(2) + (z - a[2] - t * dz).powi(2) < radius * radius {
                return true;
            }
        }
    }
    false
}

// Server-side move check: the floor below feet at `current` (which may be mid
// jump or fall), or None if a wall blocks it.
pub fn stand_height(map: Map, x: f32, z: f32, current: f32, radius: f32) -> Option<f32> {
    let floor = floor_at(map, x, z, current, STEP_UP)?;
    (!wall_blocked(map, x, z, current, radius)).then_some(floor)
}

pub fn ray_map(origin: [f32; 3], direction: [f32; 3]) -> f32 {
    let mut nearest = 100.0_f32;
    for face in faces() {
        let edge1 = sub(face.p[1], face.p[0]);
        let edge2 = sub(face.p[2], face.p[0]);
        let h = cross(direction, edge2);
        let det = dot(edge1, h);
        if det.abs() < 0.00001 {
            continue;
        }
        let inv = 1.0 / det;
        let s = sub(origin, face.p[0]);
        let u = inv * dot(s, h);
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = cross(s, edge1);
        let v = inv * dot(direction, q);
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let distance = inv * dot(edge2, q);
        if distance > 0.01 {
            nearest = nearest.min(distance);
        }
    }
    nearest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_edges_fall_and_boxes_block() {
        assert_eq!(floor_at(Map::Test, 0.0, 0.0, 0.0, STEP_UP), Some(0.0));
        assert_eq!(floor_at(Map::Test, 25.0, 0.0, 0.0, STEP_UP), None);
        assert!(wall_blocked(Map::Test, -3.0, -3.0, 0.0, 0.28));
        assert!(!wall_blocked(Map::Test, 0.0, 0.0, 0.0, 0.28));
    }

    #[test]
    fn camel_spawn_stands_on_floor() {
        let [x, y, z] = CAMEL_SPAWN;
        let floor = floor_at(Map::Camel, x, z, y, STEP_UP).expect("spawn has a floor");
        assert!((floor - y).abs() < 0.5, "spawn floor {floor} vs {y}");
    }

    #[test]
    fn walking_off_a_ledge_is_a_valid_move() {
        // Feet still at the terrace height, about 2.8 m above the floor below.
        assert!(stand_height(Map::Camel, -24.6, 22.3, 6.57, 0.28).is_some());
    }

    #[test]
    fn team_spawns_are_on_their_terraces() {
        for (area, terrace) in [
            (CAMEL_T_SPAWN, CAMEL_T_FLOOR),
            (CAMEL_CT_SPAWN, CAMEL_CT_FLOOR),
        ] {
            let x = (area[0] + area[2]) * 0.5;
            let z = (area[1] + area[3]) * 0.5;
            let floor =
                stand_height(Map::Camel, x, z, terrace, 0.28).expect("team spawn has a floor");
            assert!((floor - terrace).abs() < 0.6, "floor {floor} vs {terrace}");
        }
    }
}
