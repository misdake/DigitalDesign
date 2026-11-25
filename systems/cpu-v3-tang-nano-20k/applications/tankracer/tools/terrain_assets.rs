//! Pack source collision cells and obtain query answers from the reference game.
use std::{collections::BTreeMap, error::Error, fs, path::Path};
use tankracer_assets::{archive::Archive, terrain::TerrainData};
use tankracer_game::{geometry, sampling, terrain};

struct Page {
    words: Vec<u16>,
    meshes: BTreeMap<u32, u16>,
}
pub fn cell_bytes(data: &TerrainData, cell: usize) -> Vec<u8> {
    let mut page = pack(data, cell).words;
    page.resize(16384, 0);
    bytes(&page)
}
impl Page {
    fn reserve(&mut self, words: usize) -> u16 {
        let at = self.words.len();
        self.words.resize(at + words, 0);
        assert!(self.words.len() <= 16384, "collision cell exceeds 32 KiB");
        at as u16
    }
    fn put32(&mut self, at: u16, value: i32) {
        self.words[usize::from(at)] = value as u16;
        self.words[usize::from(at) + 1] = (value >> 16) as u16;
    }
    fn mesh(&mut self, data: &TerrainData, id: u32) -> u16 {
        if let Some(&at) = self.meshes.get(&id) {
            return at;
        }
        let mesh = &data.meshes[id as usize];
        let at = self.reserve(16);
        self.meshes.insert(id, at);
        for (axis, &value) in mesh.position.iter().enumerate() {
            self.put32(at + axis as u16 * 2, value);
        }
        self.put32(at + 6, mesh.flags as i32);
        self.words[usize::from(at + 8)] = mesh.radius;
        let vertices = self.reserve(mesh.vertices.len() * 3);
        let faces = self.reserve(mesh.triangles.len() * 6);
        let centers = if mesh.face_centers.is_empty() {
            0
        } else {
            self.reserve(mesh.face_centers.len() * 3)
        };
        for (offset, value) in [
            vertices,
            mesh.vertices.len() as u16,
            faces,
            mesh.triangles.len() as u16,
            centers,
        ]
        .into_iter()
        .enumerate()
        {
            self.words[usize::from(at) + 9 + offset] = value;
        }
        let face_source = mesh.triangles.first().map_or(0, |face| face.source_offset);
        self.put32(at + 14, face_source as i32);
        for (index, vertex) in mesh.vertices.iter().enumerate() {
            for (axis, &value) in vertex.iter().enumerate() {
                self.words[usize::from(vertices) + index * 3 + axis] = value as u16;
            }
        }
        for (index, face) in mesh.triangles.iter().enumerate() {
            assert_eq!(face.source_offset, face_source + index as u32 * 24);
            let base = faces + index as u16 * 6;
            for (axis, &value) in face.normal.iter().enumerate() {
                self.words[usize::from(base) + axis] = value as u16;
            }
            self.words[usize::from(base + 3)] =
                u16::from(face.indices[0]) | u16::from(face.indices[1]) << 8;
            self.words[usize::from(base + 4)] = u16::from(face.indices[2]);
            self.words[usize::from(base + 5)] = face.flags;
        }
        for (index, center) in mesh.face_centers.iter().enumerate() {
            for (axis, &value) in center.iter().enumerate() {
                self.words[usize::from(centers) + index * 3 + axis] = value as u16;
            }
        }
        at
    }
    fn candidates(&mut self, data: &TerrainData, start: u32, length: u32) -> u16 {
        let at = self.reserve((length as usize).div_ceil(2));
        // Preserve original byte indices, byte offsets and sentinel order.
        for (index, &value) in data.triangle_refs[start as usize..(start + length) as usize]
            .iter()
            .enumerate()
        {
            self.words[usize::from(at) + index / 2] |= u16::from(value) << ((index & 1) * 8);
        }
        at
    }
}

fn pack(data: &TerrainData, cell: usize) -> Page {
    let mut page = Page {
        words: vec![0; 16],
        meshes: BTreeMap::new(),
    };
    let primary = data.cells[cell];
    let fallback = data.fallback.cells[cell];
    let directory = page.reserve(primary.length as usize);
    let secondary = page.reserve(fallback.length as usize);
    page.words[0] = 0x5443;
    page.words[1] = 1;
    page.words[2] = cell as u16;
    page.put32(4, data.origin[0]);
    page.put32(6, data.origin[1]);
    page.words[8] = u16::from(data.shifts[0]) | u16::from(data.shifts[1]) << 8;
    page.words[9] = primary.length as u16;
    page.words[10] = directory;
    page.words[11] = fallback.length as u16;
    page.words[12] = secondary;
    for (index, &id) in data.node_refs
        [primary.start as usize..(primary.start + primary.length) as usize]
        .iter()
        .enumerate()
    {
        let node = data.node(id);
        let at = page.reserve(16);
        page.words[usize::from(directory) + index] = at;
        let mesh = page.mesh(data, node.mesh);
        let bins = page.reserve(64);
        let candidates = page.candidates(data, node.candidates.start, node.candidates.length);
        let fields = [
            mesh,
            bins,
            candidates,
            node.candidates.length as u16,
            u16::from(node.shifts[0]) | u16::from(node.shifts[1]) << 8,
            node.half_bounds[0] as u16,
            node.half_bounds[1] as u16,
            node.surface,
            node.extra[0] as u16,
            node.extra[1] as u16,
            node.extra[2] as u16,
            node.extra[3] as u16,
            u16::from(node.has_owner),
        ];
        page.words[usize::from(at)..usize::from(at) + fields.len()].copy_from_slice(&fields);
        page.put32(at + 13, node.source_offset as i32);
        for (offset, &value) in node.bins.iter().enumerate() {
            page.words[usize::from(bins) + offset] = value as u16;
        }
    }
    for (index, &id) in data.fallback.node_refs
        [fallback.start as usize..(fallback.start + fallback.length) as usize]
        .iter()
        .enumerate()
    {
        let node = &data.fallback.nodes[id as usize];
        let at = page.reserve(16);
        page.words[usize::from(secondary) + index] = at;
        let mesh = page.mesh(data, node.mesh);
        let candidates = page.candidates(data, node.candidates.start, node.candidates.length);
        page.words[usize::from(at)..usize::from(at) + 3].copy_from_slice(&[
            mesh,
            candidates,
            node.candidates.length as u16,
        ]);
        for (offset, &value) in node.basis.iter().chain(&node.bounds).enumerate() {
            page.words[usize::from(at) + 3 + offset] = value as u16;
        }
        page.put32(at + 11, node.source_offset as i32);
    }
    page.words[3] = page.words.len() as u16;
    page
}

fn push32(words: &mut Vec<u16>, value: i32) {
    words.extend([value as u16, (value >> 16) as u16]);
}
fn status(result: Result<bool, terrain::Error>) -> u16 {
    match result {
        Ok(false) => 0,
        Ok(true) => 1,
        Err(terrain::Error::Arithmetic) => 2,
        Err(terrain::Error::InvalidBucket) => 3,
    }
}
fn sample(data: &TerrainData, world: [i32; 3], mode: i32, words: &mut Vec<u16>) {
    for value in world {
        push32(words, value);
    }
    words.push(mode as u16);
    let mut hit = terrain::Hit {
        height: -55,
        node: None,
        normal: [-12, -13, -14],
        surface: 0x8899aabb,
        special: -77,
        extra: [19, 23, -29, 31],
        flags: 0xabcd1234,
    };
    let full_status = status(terrain::query(data, world[0], world[2], &mut hit));
    words.push(full_status);
    let face = if full_status == 1 {
        terrain::select(data, world[0], world[2])
            .unwrap()
            .unwrap()
            .triangle
            .source_offset as i32
    } else {
        0x55667788
    };
    let node = hit
        .node
        .map_or(0x11223344, |id| data.node(id).source_offset as i32);
    for value in [
        hit.height,
        node,
        face,
        hit.normal[0],
        hit.normal[1],
        hit.normal[2],
        hit.surface as i32,
        hit.special,
        hit.extra[0],
        hit.extra[1],
        hit.extra[2],
        hit.extra[3],
        hit.flags as i32,
    ] {
        push32(words, value);
    }
    let mut height = 0x12345678;
    words.push(status(terrain::height(
        data,
        world[0],
        world[2],
        &mut height,
    )));
    push32(words, height);
    let mut normal = [0x11111111, 0x22222222, 0x33333333];
    height = 0x12345678;
    words.push(status(terrain::normal_height(
        data,
        world[0],
        world[2],
        &mut height,
        &mut normal,
    )));
    push32(words, height);
    for value in normal {
        push32(words, value);
    }
    normal = [0x11111111, 0x22222222, 0x33333333];
    words.push(u16::from(geometry::fallback(
        data,
        world,
        mode,
        &mut normal,
    )));
    for value in normal {
        push32(words, value);
    }
    let transform = sampling::Transform {
        basis: [0, 0, -4096, 0, 4096, 0, 4096, 0, 0],
        origin: [
            world[0].wrapping_sub(91),
            world[1].wrapping_add(77),
            world[2].wrapping_add(33),
        ],
    };
    let mut auxiliary = sampling::AuxiliaryContact {
        world: [-1, -2, -3],
        kind: -11,
        surface: -12,
        cached: -13,
    };
    words.push(status(auxiliary.sample(
        data,
        [33, -77, 91],
        &transform,
        mode & 1 != 0,
    )));
    for value in
        auxiliary
            .world
            .into_iter()
            .chain([auxiliary.kind, auxiliary.surface, auxiliary.cached])
    {
        push32(words, value);
    }
}

fn bytes(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn centroids(data: &TerrainData, cell: usize) -> Vec<[i32; 3]> {
    let mut points = Vec::new();
    let list = data.cells[cell];
    for &id in &data.node_refs[list.start as usize..(list.start + list.length) as usize] {
        let mesh = &data.meshes[data.node(id).mesh as usize];
        for face in &mesh.triangles {
            let p = face.indices.map(|i| mesh.vertices[i as usize]);
            let world = std::array::from_fn(|axis| {
                (mesh.position[axis] + p.iter().map(|v| i32::from(v[axis])).sum::<i32>() / 3) * 8
            });
            let gx = ((world[0] >> 3) - data.origin[0]) >> data.shifts[0];
            let gz = ((world[2] >> 3) - data.origin[1]) >> data.shifts[1];
            if (0..8).contains(&gx) && (0..8).contains(&gz) && gx + 8 * gz == cell as i32 {
                points.push(world);
            }
        }
    }
    points
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("terrain-assets ARCHIVE CELL_A,CELL_B OUTPUT".into());
    }
    let source = fs::read(&args[0])?;
    let archive = Archive::parse(&source)?;
    let bsm = archive.get("ENGLAND.BSM").ok_or("missing ENGLAND.BSM")?;
    let data = TerrainData::decode_bsm(bsm)?;
    let cells: Vec<usize> = args[1]
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    if cells.len() != 2
        || cells.iter().any(|&cell| cell >= 64)
        || !(cells[0] / 8 == cells[1] / 8 && cells[0].abs_diff(cells[1]) == 1
            || cells[0].abs_diff(cells[1]) == 8)
    {
        return Err("two adjacent cells required".into());
    }
    let out = Path::new(&args[2]);
    fs::create_dir_all(out)?;
    fs::write(out.join("source.bsm"), bsm)?;
    let mut sizes = String::new();
    for cell in 0..64 {
        let page = pack(&data, cell);
        sizes += &format!("{cell},{}\n", page.words.len() * 2);
        if cells.contains(&cell) {
            let mut padded = page.words;
            padded.resize(16384, 0);
            fs::write(out.join(format!("cell-{cell}.bin")), bytes(&padded))?;
        }
    }
    fs::write(out.join("cell-sizes.csv"), sizes)?;
    let mut queries = Vec::new();
    let seeds = [centroids(&data, cells[0]), centroids(&data, cells[1])];
    for index in 0..8 {
        for points in &seeds {
            if !points.is_empty() {
                sample(
                    &data,
                    points[index % points.len()],
                    (index % 3) as i32,
                    &mut queries,
                );
            }
        }
    }
    for &cell in &cells {
        let start = [
            data.origin[0] + (((cell % 8) as i32) << data.shifts[0]),
            data.origin[1] + (((cell / 8) as i32) << data.shifts[1]),
        ];
        let span = [1i32 << data.shifts[0], 1i32 << data.shifts[1]];
        for z in 0..=8 {
            for x in 0..=8 {
                let world = [
                    (start[0] + (span[0] - 1) * x / 8) * 8,
                    if (x + z) & 1 == 0 { 0 } else { 10000 },
                    (start[1] + (span[1] - 1) * z / 8) * 8,
                ];
                for mode in 0..3 {
                    sample(&data, world, mode, &mut queries);
                }
            }
        }
        // Original candidate centroids exercise the faces actually used by this cell.
        let points = centroids(&data, cell);
        for &world in &points[..points.len().min(256)] {
            sample(&data, world, 0, &mut queries);
        }
    }
    let axis = usize::from(cells[0] / 8 != cells[1] / 8);
    let other = 1 - axis;
    let grid = [cells[0] % 8, cells[0] / 8];
    let origins = data.origin;
    let boundary = (origins[axis] + ((grid[axis] as i32 + 1) << data.shifts[axis])) * 8;
    for along in 0..=8 {
        for delta in [-1, 0, 1] {
            let mut world = [0; 3];
            world[axis * 2] = boundary + delta;
            world[other * 2] = (origins[other]
                + ((grid[other] as i32) << data.shifts[other])
                + ((1 << data.shifts[other]) - 1) * along / 8)
                * 8;
            for mode in 0..3 {
                sample(&data, world, mode, &mut queries);
            }
        }
    }
    for mode in 0..3 {
        sample(&data, [i32::MIN, 0, i32::MAX], mode, &mut queries);
    }
    assert_eq!(queries.len() % 66, 0);
    fs::write(out.join("queries.bin"), bytes(&queries))?;
    fs::write(
        out.join("map.txt"),
        format!(
            "{} {} {} {} {}\n",
            data.origin[0],
            data.origin[1],
            data.shifts[0],
            data.shifts[1],
            queries.len() / 66
        ),
    )?;
    println!(
        "packed cells {cells:?}; {} reference query records",
        queries.len() / 66
    );
    Ok(())
}
