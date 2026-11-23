use glam::{Mat4, Quat, Vec3};
use std::{collections::HashMap, env, error::Error, fmt::Write, fs};

struct Part {
    first: usize,
    count: usize,
    image: Option<usize>,
}

// Extracts Keles's original DE_KERVAN map GLB (GPL-3.0-or-later) into a compact
// binary vertex buffer, draw manifest, and PNG textures. The gltf-rs crate
// parses the GLB; this converter is original Rust.
fn visit_map(
    node: gltf::Node,
    parent: Mat4,
    bin: &[u8],
    vertices: &mut Vec<[f32; 5]>,
    parts: &mut Vec<Part>,
) {
    let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = primitive.reader(|_| Some(bin));
            let positions: Vec<Vec3> = reader
                .read_positions()
                .unwrap()
                .map(|p| transform.transform_point3(Vec3::from_array(p)))
                .collect();
            let tex_set = primitive
                .material()
                .pbr_metallic_roughness()
                .base_color_texture()
                .map(|info| info.tex_coord())
                .unwrap_or(0);
            let texcoords: Vec<[f32; 2]> = reader
                .read_tex_coords(tex_set)
                .map(|uv| uv.into_f32().collect())
                .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);
            let indices: Vec<usize> = reader
                .read_indices()
                .map(|i| i.into_u32().map(|i| i as usize).collect())
                .unwrap_or_else(|| (0..positions.len()).collect());
            let first = vertices.len();
            for &index in &indices {
                let p = positions[index];
                let uv = texcoords[index];
                vertices.push([p.x, p.y, p.z, uv[0], uv[1]]);
            }
            let pbr = primitive.material().pbr_metallic_roughness();
            parts.push(Part {
                first,
                count: vertices.len() - first,
                image: pbr
                    .base_color_texture()
                    .map(|info| info.texture().source().index()),
            });
        }
    }
    for child in node.children() {
        visit_map(child, transform, bin, vertices, parts);
    }
}

fn convert_map() -> Result<(), Box<dyn Error>> {
    let gltf = gltf::Gltf::open("assets/kervan_original.glb")?;
    let bin = gltf.blob.as_deref().ok_or("Missing GLB binary chunk")?;
    let mut vertices = Vec::new();
    let mut parts = Vec::new();
    for node in gltf.default_scene().ok_or("Missing scene")?.nodes() {
        visit_map(node, Mat4::IDENTITY, bin, &mut vertices, &mut parts);
    }
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for vertex in &vertices {
        min = min.min(Vec3::new(vertex[0], vertex[1], vertex[2]));
        max = max.max(Vec3::new(vertex[0], vertex[1], vertex[2]));
    }
    println!("{} triangles, min={min:?}, max={max:?}", vertices.len() / 3);
    let center = Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5);
    for vertex in &mut vertices {
        vertex[0] -= center.x;
        vertex[1] -= center.y;
        vertex[2] -= center.z;
    }
    let mut bytes = Vec::with_capacity(vertices.len() * 20);
    for vertex in &vertices {
        for value in vertex {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    fs::write("assets/kervan_mesh.bin", bytes)?;
    let _ = fs::remove_dir_all("assets/kervan_images");
    fs::create_dir_all("assets/kervan_images")?;
    let mut image_names = Vec::new();
    for image in gltf.images() {
        let gltf::image::Source::View { view, mime_type } = image.source() else {
            return Err("Expected an embedded GLB image".into());
        };
        let extension = match mime_type {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            _ => return Err(format!("Unsupported image: {mime_type}").into()),
        };
        let name = format!("{}.{}", image.index(), extension);
        let start = view.offset();
        fs::write(
            format!("assets/kervan_images/{name}"),
            &bin[start..start + view.length()],
        )?;
        image_names.push(name);
    }
    let mut manifest = String::new();
    for part in parts {
        let image = part.image.map(|i| image_names[i].as_str()).unwrap_or("-");
        writeln!(manifest, "{} {} {}", part.first, part.count, image)?;
    }
    fs::write("assets/kervan_parts.txt", &manifest)?;
    println!(
        "{} draw parts, {} images",
        manifest.lines().count(),
        image_names.len()
    );
    Ok(())
}

struct ModelPart {
    node_idx: usize,
    first: usize,
    count: usize,
    image: usize,
}

struct ModelNode {
    parent: Option<usize>,
    name: String,
    t: [f32; 3],
    r: [f32; 4],
    s: [f32; 3],
}

fn glb_node_names(glb_bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    if glb_bytes.len() < 20 {
        return names;
    }
    let json_len = u32::from_le_bytes(glb_bytes[12..16].try_into().unwrap()) as usize;
    let Ok(json) = std::str::from_utf8(glb_bytes.get(20..20 + json_len).unwrap_or(&[])) else {
        return names;
    };
    let mut search = json;
    let rest = loop {
        let Some(key_pos) = search.find("\"nodes\"") else {
            return names;
        };
        let after_key = &search[key_pos + 7..];
        let Some(arr_pos) = after_key.find('[') else {
            return names;
        };
        let candidate = after_key[arr_pos + 1..].trim_start();
        if candidate.starts_with('{') {
            break candidate;
        }
        search = &after_key[arr_pos + 1..];
    };
    let mut depth = 0i32;
    let mut obj_start = None;
    for (i, ch) in rest.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    obj_start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = obj_start.take() {
                        let obj = &rest[s..=i];
                        let name = obj
                            .find("\"name\"")
                            .and_then(|p| {
                                let after = &obj[p + 6..];
                                let colon = after.find(':')?;
                                let after_colon = &after[colon + 1..];
                                let q1 = after_colon.find('"')?;
                                let val = &after_colon[q1 + 1..];
                                let q2 = val.find('"')?;
                                Some(val[..q2].to_owned())
                            })
                            .unwrap_or_else(|| format!("node_{}", names.len()));
                        names.push(name);
                    }
                }
            }
            ']' if depth == 0 => break,
            _ => {}
        }
    }
    names
}

fn visit_model(
    node: gltf::Node,
    parent: Option<usize>,
    bin: &[u8],
    node_names: &[String],
    nodes: &mut Vec<ModelNode>,
    vertices: &mut Vec<[f32; 8]>,
    parts: &mut Vec<ModelPart>,
    index_map: &mut HashMap<usize, usize>,
) {
    let (t, r, s) = node.transform().decomposed();
    let q = Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize();
    let my_idx = nodes.len();
    index_map.insert(node.index(), my_idx);
    let raw_name = node_names
        .get(node.index())
        .map(String::as_str)
        .unwrap_or("node")
        .replace(char::is_whitespace, "_");
    nodes.push(ModelNode {
        parent,
        name: raw_name,
        t,
        r: [q.x, q.y, q.z, q.w],
        s,
    });
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = primitive.reader(|_| Some(bin));
            let positions: Vec<[f32; 3]> = reader.read_positions().unwrap().collect();
            let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
            let tex_set = primitive
                .material()
                .pbr_metallic_roughness()
                .base_color_texture()
                .map(|info| info.tex_coord())
                .unwrap_or(0);
            let texcoords: Vec<[f32; 2]> = reader
                .read_tex_coords(tex_set)
                .map(|uv| uv.into_f32().collect())
                .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);
            let indices: Vec<usize> = reader
                .read_indices()
                .map(|i| i.into_u32().map(|i| i as usize).collect())
                .unwrap_or_else(|| (0..positions.len()).collect());
            let first = vertices.len();
            for tri in indices.chunks_exact(3) {
                let p0 = Vec3::from_array(positions[tri[0]]);
                let p1 = Vec3::from_array(positions[tri[1]]);
                let p2 = Vec3::from_array(positions[tri[2]]);
                let face_n = (p1 - p0).cross(p2 - p0).normalize_or_zero();
                for &idx in tri {
                    let p = positions[idx];
                    let n = normals
                        .as_ref()
                        .map(|ns| Vec3::from_array(ns[idx]).normalize_or_zero())
                        .unwrap_or(face_n);
                    let uv = texcoords[idx];
                    vertices.push([p[0], p[1], p[2], n.x, n.y, n.z, uv[0], uv[1]]);
                }
            }
            let image = primitive
                .material()
                .pbr_metallic_roughness()
                .base_color_texture()
                .map(|info| info.texture().source().index())
                .unwrap_or(0);
            parts.push(ModelPart {
                node_idx: my_idx,
                first,
                count: vertices.len() - first,
                image,
            });
        }
    }
    for child in node.children() {
        visit_model(
            child,
            Some(my_idx),
            bin,
            node_names,
            nodes,
            vertices,
            parts,
            index_map,
        );
    }
}

fn convert_model(glb_path: &str, out_prefix: &str) -> Result<(), Box<dyn Error>> {
    let raw = fs::read(glb_path)?;
    let node_names = glb_node_names(&raw);
    let gltf = gltf::Gltf::from_slice(&raw)?;
    let bin = gltf.blob.as_deref().ok_or("Missing GLB binary chunk")?;
    let mut nodes = Vec::new();
    let mut vertices = Vec::new();
    let mut parts = Vec::new();
    let mut index_map = HashMap::new();
    for node in gltf.default_scene().ok_or("Missing scene")?.nodes() {
        visit_model(
            node,
            None,
            bin,
            &node_names,
            &mut nodes,
            &mut vertices,
            &mut parts,
            &mut index_map,
        );
    }
    let mut bytes = Vec::with_capacity(vertices.len() * 32);
    for v in &vertices {
        for f in v {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
    }
    fs::write(format!("{out_prefix}_mesh.bin"), bytes)?;
    let img_dir = format!("{out_prefix}_images");
    let _ = fs::remove_dir_all(&img_dir);
    fs::create_dir_all(&img_dir)?;
    let mut image_count = 0usize;
    for image in gltf.images() {
        let gltf::image::Source::View { view, mime_type } = image.source() else {
            return Err("Expected an embedded GLB image".into());
        };
        if mime_type != "image/png" {
            return Err(format!("Expected PNG image, got {mime_type}").into());
        }
        let start = view.offset();
        fs::write(
            format!("{img_dir}/{}.png", image.index()),
            &bin[start..start + view.length()],
        )?;
        image_count += 1;
    }
    let mut manifest = String::new();
    for (idx, n) in nodes.iter().enumerate() {
        let parent = n.parent.map(|p| p as isize).unwrap_or(-1);
        writeln!(
            manifest,
            "N {} {} {} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6}",
            idx,
            parent,
            n.name,
            n.t[0],
            n.t[1],
            n.t[2],
            n.r[0],
            n.r[1],
            n.r[2],
            n.r[3],
            n.s[0],
            n.s[1],
            n.s[2]
        )?;
    }
    for p in &parts {
        writeln!(
            manifest,
            "P {} {} {} {}",
            p.node_idx, p.first, p.count, p.image
        )?;
    }
    fs::write(format!("{out_prefix}_nodes.txt"), &manifest)?;
    println!(
        "{glb_path} -> {} tris, {} nodes, {} parts, {} images",
        vertices.len() / 3,
        nodes.len(),
        parts.len(),
        image_count
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() == 4 && args[1] == "--model" {
        convert_model(&args[2], &args[3])
    } else {
        convert_map()
    }
}
