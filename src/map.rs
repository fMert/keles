// Original DE_KERVAN map geometry and collision (GPL-3.0-or-later).
// Extracted from assets/kervan_original.glb by tools/convert.
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq)]
pub enum Map {
    Test,
    Kervan,
}

// DE_KERVAN is authored directly in world meters (1.0 = 1 m).
pub const KERVAN_SCALE: f32 = 1.0;

// Kills a team needs to win a round; the scoreboard shows it too.
pub const WIN_SCORE: u32 = 40;

pub const KERVAN_SPAWN: [f32; 3] = [-10.0, 6.0, 34.0];

// Team spawn areas on DE_KERVAN as world-space [x0, z0, x1, z1] rectangles,
// plus each terrace's floor height. T spawn is the raised southern terrace;
// CT spawn is the lower northern underpass beneath the A-Short overpass.
pub const KERVAN_T_SPAWN: [f32; 4] = [-15.5, 32.0, -4.5, 36.0];
pub const KERVAN_T_FLOOR: f32 = 6.0;
pub const KERVAN_CT_SPAWN: [f32; 4] = [6.5, -25.5, 13.8, -21.0];
pub const KERVAN_CT_FLOOR: f32 = 1.2;

// Connected Dust2-style lane waypoints (x, z) and neighbor indices for practice bots.
pub const KERVAN_WAYPOINTS: &[([f32; 2], &[usize])] = &[
    ([-10.0, 34.0], &[1, 26, 31]),    // 0 T Spawn
    ([0.0, 32.0], &[0, 2]),           // 1 T Mid Lip
    ([0.0, 22.0], &[1, 3]),           // 2 Suicide
    ([0.0, 10.5], &[2, 4, 15, 30]),   // 3 Top Mid
    ([11.5, 13.0], &[3, 5, 6]),       // 4 Outside Long South
    ([14.0, 20.0], &[4, 31]),         // 5 Outside Long North
    ([19.0, 13.0], &[4, 7]),          // 6 Long Gate
    ([25.5, 11.0], &[6, 8]),          // 7 Long Doors Exit
    ([26.0, 4.5], &[7, 9]),           // 8 Long Corner
    ([29.5, -2.0], &[8, 10]),         // 9 Long A Mid
    ([29.5, -17.0], &[9, 11, 19]),    // 10 A Ramp
    ([28.0, -27.0], &[10, 12]),       // 11 A Site
    ([13.0, -25.0], &[11, 13]),       // 12 A Short Overpass
    ([12.5, -10.0], &[12, 14]),       // 13 Catwalk East
    ([6.5, -6.0], &[13, 15]),         // 14 Short Stairs Top
    ([-1.0, -5.0], &[3, 14, 16, 29]), // 15 Lower Mid
    ([0.0, -16.5], &[15, 17]),        // 16 Mid Doors
    ([0.0, -24.0], &[16, 18, 20]),    // 17 CT Mid
    ([9.5, -24.0], &[17, 19]),        // 18 CT Spawn
    ([19.5, -19.0], &[10, 18]),       // 19 CT-A Ramp
    ([-11.0, -25.0], &[17, 21]),      // 20 CT-B Ramp
    ([-17.0, -26.0], &[20, 22]),      // 21 B Doors
    ([-24.5, -23.0], &[21, 23]),      // 22 B Site
    ([-28.0, -12.0], &[22, 27]),      // 23 B Tunnel Exit
    ([-28.0, 10.0], &[25, 27, 30]),   // 24 Upper Tunnels
    ([-29.0, 25.0], &[24, 26]),       // 25 Outside Tunnels
    ([-24.0, 33.0], &[0, 25]),        // 26 T West Ramp
    ([-28.0, -1.5], &[23, 24, 28]),   // 27 Upper-Lower Corner
    ([-19.5, -1.5], &[27, 29]),       // 28 Lower Tunnels West
    ([-10.0, -1.5], &[15, 28]),       // 29 Lower Tunnels East
    ([-14.0, 12.5], &[3, 24]),        // 30 Upper to Top Mid
    ([11.5, 33.0], &[0, 5]),          // 31 T East Ramp
];

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
        Map::Kervan => &[],
    }
}

struct Face {
    p: [[f32; 3]; 3],
    normal_y: f32,
    min: [f32; 3],
    max: [f32; 3],
}

const GRID_X: usize = 40;
const GRID_Z: usize = 42;
const CELL: f32 = 2.0;
const ORIGIN_X: f32 = -40.0;
const ORIGIN_Z: f32 = -42.0;

struct MapGrid {
    faces: Vec<Face>,
    floors: Vec<Vec<u32>>,
    walls: Vec<Vec<u32>>,
    all: Vec<Vec<u32>>,
}

static GRID: OnceLock<MapGrid> = OnceLock::new();

fn cell_range(min: f32, max: f32, origin: f32, count: usize) -> (usize, usize) {
    let lo = ((min - origin) / CELL)
        .floor()
        .clamp(0.0, (count - 1) as f32) as usize;
    let hi = ((max - origin) / CELL)
        .floor()
        .clamp(0.0, (count - 1) as f32) as usize;
    (lo, hi)
}

fn grid() -> &'static MapGrid {
    GRID.get_or_init(|| {
        let bytes = include_bytes!("../assets/kervan_mesh.bin");
        let mut vertices = Vec::with_capacity(bytes.len() / 20);
        for vertex in bytes.as_chunks::<20>().0 {
            let coordinate =
                |i| f32::from_le_bytes(vertex[i..i + 4].try_into().unwrap()) * KERVAN_SCALE;
            vertices.push([coordinate(0), coordinate(4), coordinate(8)]);
        }
        let faces: Vec<Face> = vertices
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
            .collect();
        let total = GRID_X * GRID_Z;
        let mut floors = vec![Vec::new(); total];
        let mut walls = vec![Vec::new(); total];
        let mut all = vec![Vec::new(); total];
        for (idx, face) in faces.iter().enumerate() {
            let id = idx as u32;
            let (x0, x1) = cell_range(face.min[0] - 0.01, face.max[0] + 0.01, ORIGIN_X, GRID_X);
            let (z0, z1) = cell_range(face.min[2] - 0.01, face.max[2] + 0.01, ORIGIN_Z, GRID_Z);
            for gz in z0..=z1 {
                for gx in x0..=x1 {
                    let cell = gz * GRID_X + gx;
                    all[cell].push(id);
                    if face.normal_y.abs() >= 0.65 {
                        floors[cell].push(id);
                    }
                }
            }
            if face.normal_y.abs() <= 0.6 {
                let (wx0, wx1) =
                    cell_range(face.min[0] - 0.35, face.max[0] + 0.35, ORIGIN_X, GRID_X);
                let (wz0, wz1) =
                    cell_range(face.min[2] - 0.35, face.max[2] + 0.35, ORIGIN_Z, GRID_Z);
                for gz in wz0..=wz1 {
                    for gx in wx0..=wx1 {
                        walls[gz * GRID_X + gx].push(id);
                    }
                }
            }
        }
        MapGrid {
            faces,
            floors,
            walls,
            all,
        }
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
    if x.abs() > 38.5 * KERVAN_SCALE || z.abs() > 40.5 * KERVAN_SCALE {
        return None;
    }
    let g = grid();
    let gx = ((x - ORIGIN_X) / CELL).floor() as isize;
    let gz = ((z - ORIGIN_Z) / CELL).floor() as isize;
    if !(0..GRID_X as isize).contains(&gx) || !(0..GRID_Z as isize).contains(&gz) {
        return None;
    }
    let mut floor: Option<f32> = None;
    for &id in &g.floors[gz as usize * GRID_X + gx as usize] {
        let face = &g.faces[id as usize];
        if x < face.min[0] || x > face.max[0] || z < face.min[2] || z > face.max[2] {
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

// Distance from (x, z) to the nearest near-vertical face that rises above the
// climbable step, capped at `radius`; negative inside a Test box.
pub fn wall_gap(map: Map, x: f32, z: f32, feet: f32, radius: f32) -> f32 {
    let mut gap = radius;
    if map == Map::Test {
        for block in TEST.iter().skip(1) {
            if block.center[1] + block.half[1] <= feet + STEP_UP {
                continue;
            }
            let dx = (x - block.center[0]).abs() - block.half[0];
            let dz = (z - block.center[2]).abs() - block.half[2];
            let outside = dx.max(0.0).hypot(dz.max(0.0));
            gap = gap.min(if outside > 0.0 { outside } else { dx.max(dz) });
        }
        return gap;
    }
    let g = grid();
    let extra = (radius - 0.35).max(0.0);
    let (x0, x1) = cell_range(x - extra, x + extra, ORIGIN_X, GRID_X);
    let (z0, z1) = cell_range(z - extra, z + extra, ORIGIN_Z, GRID_Z);
    for gz in z0..=z1 {
        for gx in x0..=x1 {
            for &id in &g.walls[gz * GRID_X + gx] {
                let face = &g.faces[id as usize];
                if face.max[1] <= feet + STEP_UP
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
                    let t = (((x - a[0]) * dx + (z - a[2]) * dz)
                        / (dx * dx + dz * dz).max(0.00001))
                    .clamp(0.0, 1.0);
                    gap = gap.min((x - a[0] - t * dx).hypot(z - a[2] - t * dz));
                }
            }
        }
    }
    gap
}

pub fn wall_blocked(map: Map, x: f32, z: f32, feet: f32, radius: f32) -> bool {
    wall_gap(map, x, z, feet, radius) < radius
}

// Server-side move check: the floor below feet at `current` (which may be mid
// jump or fall), or None if a wall blocks it.
pub fn stand_height(map: Map, x: f32, z: f32, current: f32, radius: f32) -> Option<f32> {
    let floor = floor_at(map, x, z, current, STEP_UP)?;
    (!wall_blocked(map, x, z, current, radius)).then_some(floor)
}

fn ray_face(face: &Face, origin: [f32; 3], direction: [f32; 3]) -> Option<f32> {
    let edge1 = sub(face.p[1], face.p[0]);
    let edge2 = sub(face.p[2], face.p[0]);
    let h = cross(direction, edge2);
    let det = dot(edge1, h);
    if det.abs() < 0.00001 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(origin, face.p[0]);
    let u = inv * dot(s, h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, edge1);
    let v = inv * dot(direction, q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = inv * dot(edge2, q);
    (distance > 0.01).then_some(distance)
}

pub fn ray_map(origin: [f32; 3], direction: [f32; 3]) -> f32 {
    let g = grid();
    let max_x = ORIGIN_X + GRID_X as f32 * CELL;
    let max_z = ORIGIN_Z + GRID_Z as f32 * CELL;
    let mut t_enter = 0.0_f32;
    let mut t_exit = 100.0_f32;
    for (o, d, lo, hi) in [
        (origin[0], direction[0], ORIGIN_X, max_x),
        (origin[2], direction[2], ORIGIN_Z, max_z),
    ] {
        if d.abs() < 1e-6 {
            if o < lo || o > hi {
                return 100.0;
            }
        } else {
            let a = (lo - o) / d;
            let b = (hi - o) / d;
            t_enter = t_enter.max(a.min(b));
            t_exit = t_exit.min(a.max(b));
            if t_enter > t_exit {
                return 100.0;
            }
        }
    }
    let sx = origin[0] + direction[0] * t_enter;
    let sz = origin[2] + direction[2] * t_enter;
    let mut gx = (((sx - ORIGIN_X) / CELL).floor() as isize).clamp(0, GRID_X as isize - 1);
    let mut gz = (((sz - ORIGIN_Z) / CELL).floor() as isize).clamp(0, GRID_Z as isize - 1);
    let (step_x, mut t_max_x, t_delta_x) = if direction[0] > 1e-6 {
        let next_x = ORIGIN_X + (gx + 1) as f32 * CELL;
        (
            1isize,
            (next_x - origin[0]) / direction[0],
            CELL / direction[0],
        )
    } else if direction[0] < -1e-6 {
        let next_x = ORIGIN_X + gx as f32 * CELL;
        (
            -1isize,
            (next_x - origin[0]) / direction[0],
            -CELL / direction[0],
        )
    } else {
        (0isize, f32::INFINITY, f32::INFINITY)
    };
    let (step_z, mut t_max_z, t_delta_z) = if direction[2] > 1e-6 {
        let next_z = ORIGIN_Z + (gz + 1) as f32 * CELL;
        (
            1isize,
            (next_z - origin[2]) / direction[2],
            CELL / direction[2],
        )
    } else if direction[2] < -1e-6 {
        let next_z = ORIGIN_Z + gz as f32 * CELL;
        (
            -1isize,
            (next_z - origin[2]) / direction[2],
            -CELL / direction[2],
        )
    } else {
        (0isize, f32::INFINITY, f32::INFINITY)
    };

    let mut nearest = 100.0_f32;
    let mut t_cell = t_enter;
    while t_cell <= t_exit.min(nearest) {
        for &id in &g.all[gz as usize * GRID_X + gx as usize] {
            if let Some(d) = ray_face(&g.faces[id as usize], origin, direction) {
                nearest = nearest.min(d);
            }
        }
        if t_max_x < t_max_z {
            t_cell = t_max_x;
            gx += step_x;
            t_max_x += t_delta_x;
            if !(0..GRID_X as isize).contains(&gx) {
                break;
            }
        } else {
            if !t_max_z.is_finite() {
                break;
            }
            t_cell = t_max_z;
            gz += step_z;
            t_max_z += t_delta_z;
            if !(0..GRID_Z as isize).contains(&gz) {
                break;
            }
        }
    }
    nearest
}

// Adapted from aevyrie/bevy_mod_raycast/src/primitives.rs, intersects_aabb,
// lines 164-203: https://github.com/aevyrie/bevy_mod_raycast/blob/main/src/primitives.rs
// License: MIT (see server/LICENSE-MIT-bevy_mod_raycast). World-space slab test, with
// a parallel-axis check so a ray beside a box cannot hit it.
pub fn ray_box(origin: [f32; 3], direction: [f32; 3], min: [f32; 3], max: [f32; 3]) -> Option<f32> {
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

static RADAR_CELLS: OnceLock<Vec<[f32; 2]>> = OnceLock::new();

// Walkable floor footprint sampled from DE_KERVAN's geometry for the tactical radar.
pub fn radar_cells() -> &'static [[f32; 2]] {
    RADAR_CELLS.get_or_init(|| {
        const GRID_N: usize = 56;
        let mut occupied = [false; GRID_N * GRID_N];
        for face in &grid().faces {
            if face.normal_y.abs() < 0.65 || face.min[1] > 6.8 {
                continue;
            }
            let x0 = (((face.min[0] + 38.0) / 76.0) * GRID_N as f32)
                .floor()
                .clamp(0.0, (GRID_N - 1) as f32) as usize;
            let x1 = (((face.max[0] + 38.0) / 76.0) * GRID_N as f32)
                .ceil()
                .clamp(0.0, (GRID_N - 1) as f32) as usize;
            let z0 = (((face.min[2] + 40.0) / 80.0) * GRID_N as f32)
                .floor()
                .clamp(0.0, (GRID_N - 1) as f32) as usize;
            let z1 = (((face.max[2] + 40.0) / 80.0) * GRID_N as f32)
                .ceil()
                .clamp(0.0, (GRID_N - 1) as f32) as usize;
            for gz in z0..=z1 {
                let wz = -40.0 + (gz as f32 + 0.5) * (80.0 / GRID_N as f32);
                for gx in x0..=x1 {
                    let wx = -38.0 + (gx as f32 + 0.5) * (76.0 / GRID_N as f32);
                    if !occupied[gz * GRID_N + gx] && floor_y(face, wx, wz).is_some() {
                        occupied[gz * GRID_N + gx] = true;
                    }
                }
            }
        }
        let mut cells = Vec::new();
        for gz in 0..GRID_N {
            for gx in 0..GRID_N {
                if occupied[gz * GRID_N + gx] {
                    cells.push([
                        -38.0 + (gx as f32 + 0.5) * (76.0 / GRID_N as f32),
                        -40.0 + (gz as f32 + 0.5) * (80.0 / GRID_N as f32),
                    ]);
                }
            }
        }
        cells
    })
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
    fn kervan_spawn_stands_on_floor() {
        let [x, y, z] = KERVAN_SPAWN;
        let floor = floor_at(Map::Kervan, x, z, y, STEP_UP).expect("spawn has a floor");
        assert!((floor - y).abs() < 0.25, "spawn floor {floor} vs {y}");
    }

    #[test]
    fn walking_off_a_ledge_is_a_valid_move() {
        // Stepping from T Spawn terrace (y=6.0) down toward Suicide (y=3.6).
        assert!(stand_height(Map::Kervan, 0.0, 26.5, 6.0, 0.28).is_some());
    }

    #[test]
    fn team_spawns_are_on_their_terraces() {
        for (area, terrace) in [
            (KERVAN_T_SPAWN, KERVAN_T_FLOOR),
            (KERVAN_CT_SPAWN, KERVAN_CT_FLOOR),
        ] {
            for &fx in &[0.1, 0.5, 0.9] {
                for &fz in &[0.1, 0.5, 0.9] {
                    let x = area[0] + fx * (area[2] - area[0]);
                    let z = area[1] + fz * (area[3] - area[1]);
                    let floor = stand_height(Map::Kervan, x, z, terrace, 0.28)
                        .expect("team spawn has a floor");
                    assert!((floor - terrace).abs() < 0.3, "floor {floor} vs {terrace}");
                }
            }
        }
    }

    #[test]
    fn t_spawn_to_ct_mid_sniper_sightline_is_clear() {
        let origin = [0.0, 7.7, 30.0];
        let target = [0.0, 2.5, -23.0];
        let delta = sub(target, origin);
        let dist = dot(delta, delta).sqrt();
        let dir = [delta[0] / dist, delta[1] / dist, delta[2] / dist];
        let hit = ray_map(origin, dir);
        assert!(hit >= dist - 1.0, "Mid sightline blocked at {hit} < {dist}");
    }

    #[test]
    fn spatial_grid_ray_map_matches_bruteforce() {
        let g = grid();
        let samples = [
            ([-10.0, 7.7, 34.0], [1.0, 0.0, 0.0]),
            ([0.0, 7.7, 30.0], [0.0, -0.095, -0.995]),
            ([9.5, 2.9, -24.0], [-1.0, 0.0, 0.0]),
            ([28.0, 7.1, -27.0], [0.0, -0.1, 0.995]),
            ([-24.5, 5.3, -23.0], [0.0, -1.0, 0.0]),
            ([29.5, 5.3, -2.0], [0.6, 0.2, -0.77]),
        ];
        for (origin, raw_dir) in samples {
            let len = dot(raw_dir, raw_dir).sqrt();
            let dir = [raw_dir[0] / len, raw_dir[1] / len, raw_dir[2] / len];
            let fast = ray_map(origin, dir);
            let mut brute = 100.0_f32;
            for face in &g.faces {
                if let Some(d) = ray_face(face, origin, dir) {
                    brute = brute.min(d);
                }
            }
            assert!(
                (fast - brute).abs() < 0.002,
                "ray mismatch for {origin:?} -> {dir:?}: fast={fast} vs brute={brute}"
            );
        }
    }

    #[test]
    fn radar_cells_and_practice_waypoints_cover_walkable_kervan_floors() {
        let cells = radar_cells();
        assert!(cells.len() > 200, "expected walkable DE_KERVAN radar cells");
        for (i, &(pos, neighbors)) in KERVAN_WAYPOINTS.iter().enumerate() {
            assert!(!neighbors.is_empty());
            let start_y = floor_at(Map::Kervan, pos[0], pos[1], 6.8, STEP_UP)
                .unwrap_or_else(|| panic!("waypoint {i} {pos:?} must sit on a walkable floor"));
            assert!(
                !wall_blocked(Map::Kervan, pos[0], pos[1], start_y, 0.28),
                "waypoint {i} {pos:?} blocked by wall"
            );
            for &n in neighbors {
                assert!(n < KERVAN_WAYPOINTS.len());
                let dst = KERVAN_WAYPOINTS[n].0;
                let dist = (dst[0] - pos[0]).hypot(dst[1] - pos[1]);
                let steps = (dist / 0.35).ceil().max(1.0) as usize;
                let mut cy = start_y;
                for s in 1..=steps {
                    let t = s as f32 / steps as f32;
                    let x = pos[0] + (dst[0] - pos[0]) * t;
                    let z = pos[1] + (dst[1] - pos[1]) * t;
                    cy = stand_height(Map::Kervan, x, z, cy, 0.28).unwrap_or_else(|| {
                        panic!("waypoint edge {i} -> {n} blocked at ({x:.2}, {cy:.2}, {z:.2})")
                    });
                }
            }
        }
    }
}
