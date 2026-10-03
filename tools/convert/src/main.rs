use glam::{Mat4, Vec3};
use std::{error::Error, fmt::Write, fs};

struct Part {
    first: usize,
    count: usize,
    image: Option<usize>,
}

// Source asset: vrchris, "de_dust2 - CS map", Sketchfab,
// https://sketchfab.com/3d-models/de-dust2-cs-map-056008d59eb849a29c0ab6884c0c3d87
// CC BY 4.0. The gltf-rs crate parses this GLB; this converter is original Rust.
fn visit(
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
                image: pbr.base_color_texture().map(|info| info.texture().source().index()),
            });
        }
    }
    for child in node.children() {
        visit(child, transform, bin, vertices, parts);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let gltf = gltf::Gltf::open("assets/camel_original.glb")?;
    let bin = gltf.blob.as_deref().ok_or("Missing GLB binary chunk")?;
    let mut vertices = Vec::new();
    let mut parts = Vec::new();
    for node in gltf.default_scene().ok_or("Missing scene")?.nodes() {
        visit(node, Mat4::IDENTITY, bin, &mut vertices, &mut parts);
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
    fs::write("assets/camel_mesh.bin", bytes)?;
    fs::create_dir_all("assets/camel_images")?;
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
        fs::write(format!("assets/camel_images/{name}"), &bin[start..start + view.length()])?;
        image_names.push(name);
    }
    let mut manifest = String::new();
    for part in parts {
        let image = part.image.map(|i| image_names[i].as_str()).unwrap_or("-");
        writeln!(
            manifest,
            "{} {} {}",
            part.first, part.count, image
        )?;
    }
    fs::write("assets/camel_parts.txt", manifest)?;
    println!("{} draw parts, {} images", gltf.meshes().count(), image_names.len());
    Ok(())
}
