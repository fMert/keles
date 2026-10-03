// Camel's mesh and textures come from the user's Sketchfab GLB (CC BY 4.0):
// https://sketchfab.com/3d-models/de-dust2-cs-map-056008d59eb849a29c0ab6884c0c3d87
use std::sync::OnceLock;
#[derive(Clone, Copy, PartialEq)]
pub enum Map {
    Test,
    Camel,
}

pub const CAMEL_SPAWN: [f32; 3] = [-20.0, 3.4, 20.0];

pub struct Block {
    pub center: [f32; 3],
    pub half: [f32; 3],
    pub color: [f32; 3],
}

pub const TEST: &[Block] = &[
    Block { center: [0.0, -0.05, 0.0], half: [20.0, 0.05, 20.0], color: [0.25, 0.46, 0.29] },
    Block { center: [-3.0, 1.0, -3.0], half: [1.0, 1.0, 1.0], color: [0.82, 0.40, 0.31] },
    Block { center: [2.0, 0.8, -5.0], half: [0.8, 0.8, 0.8], color: [0.91, 0.72, 0.31] },
    Block { center: [4.0, 1.4, 1.0], half: [0.8, 1.4, 0.8], color: [0.38, 0.55, 0.77] },
    Block { center: [-5.0, 0.6, 2.0], half: [1.2, 0.6, 0.8], color: [0.72, 0.57, 0.76] },
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
        for vertex in bytes.chunks_exact(20) {
            let coordinate = |i| f32::from_le_bytes(vertex[i..i + 4].try_into().unwrap());
            vertices.push([coordinate(0), coordinate(4), coordinate(8)]);
        }
        vertices.chunks_exact(3).map(|v| {
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
            Face { p: [v[0], v[1], v[2]], normal_y: n[1] / length, min, max }
        }).collect()
    })
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

// The current floor selects the correct level where tunnels pass under roofs.
pub fn stand_height(map: Map, x: f32, z: f32, current: f32, radius: f32) -> Option<f32> {
    if map == Map::Test {
        return (!TEST.iter().skip(1).any(|b| {
            (x - b.center[0]).abs() < b.half[0] + radius
                && (z - b.center[2]).abs() < b.half[2] + radius
        })).then_some(0.0);
    }
    if x.abs() > 30.0 || z.abs() > 36.0 { return None; }
    let mut floor = None;
    let mut best = f32::INFINITY;
    for face in faces() {
        if face.normal_y.abs() < 0.65 || x < face.min[0] || x > face.max[0]
            || z < face.min[2] || z > face.max[2] { continue; }
        let [a, b, c] = face.p;
        let denom = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
        if denom.abs() < 0.00001 { continue; }
        let u = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / denom;
        let v = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / denom;
        if u < -0.0001 || v < -0.0001 || u + v > 1.0001 { continue; }
        let y = a[1] * u + b[1] * v + c[1] * (1.0 - u - v);
        let difference = y - current;
        if difference > 0.65 || difference < -1.25 { continue; }
        if difference.abs() < best { best = difference.abs(); floor = Some(y); }
    }
    let floor = floor?;
    for face in faces() {
        if face.normal_y.abs() > 0.6 || face.max[1] < floor + 0.25
            || face.min[1] > floor + 1.7
            || x + radius < face.min[0] || x - radius > face.max[0]
            || z + radius < face.min[2] || z - radius > face.max[2] { continue; }
        for edge in 0..3 {
            let a = face.p[edge];
            let b = face.p[(edge + 1) % 3];
            let dx = b[0] - a[0];
            let dz = b[2] - a[2];
            let t = (((x - a[0]) * dx + (z - a[2]) * dz) / (dx * dx + dz * dz).max(0.00001)).clamp(0.0, 1.0);
            if (x - a[0] - t * dx).powi(2) + (z - a[2] - t * dz).powi(2) < radius * radius {
                return None;
            }
        }
    }
    Some(floor)
}

pub fn ray_map(origin: [f32; 3], direction: [f32; 3]) -> f32 {
    let mut nearest = 100.0_f32;
    for face in faces() {
        let edge1 = sub(face.p[1], face.p[0]);
        let edge2 = sub(face.p[2], face.p[0]);
        let h = cross(direction, edge2);
        let det = dot(edge1, h);
        if det.abs() < 0.00001 { continue; }
        let inv = 1.0 / det;
        let s = sub(origin, face.p[0]);
        let u = inv * dot(s, h);
        if !(0.0..=1.0).contains(&u) { continue; }
        let q = cross(s, edge1);
        let v = inv * dot(direction, q);
        if v < 0.0 || u + v > 1.0 { continue; }
        let distance = inv * dot(edge2, q);
        if distance > 0.01 { nearest = nearest.min(distance); }
    }
    nearest
}
