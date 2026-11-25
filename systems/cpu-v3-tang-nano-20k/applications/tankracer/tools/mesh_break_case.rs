fn at(data: Array<u16>, words: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(words as i16).as_u16_array() }
}
fn copy(mut to: Array<u16>, from: Array<u16>, count: u16) {
    let mut i = 0u16;
    while i < count {
        to[i] = from[i];
        i += 1;
    }
}
pub struct FragmentCase {
    pub state: mesh_break::State,
    pub mesh: mesh_break::Mesh,
    pub config: mesh_break::Config,
}
pub fn load_mesh_break(input: Array<u16>, scratch: Array<u16>) -> FragmentCase {
    let count = attitude::get(input, 38).lo_bits();
    let vertex_count = attitude::get(input, 39).lo_bits();
    let triangle_count = attitude::get(input, 40).lo_bits();
    copy(scratch, at(input, 68), 8);
    let faces = at(scratch, 8);
    copy(faces, at(input, 90), count * 16);
    let raw_vertices = at(scratch, 8 + count * 16);
    let mut points = raw_vertices;
    let mut cursor = 45 + count * 8;
    let mut i = 0u16;
    while i < vertex_count * 3 {
        points[i] = attitude::get(input, cursor).lo_bits();
        i += 1;
        cursor += 1;
    }
    let indices = at(raw_vertices, vertex_count * 3);
    let mut ix = indices;
    let flags = at(indices, triangle_count * 3);
    let mut fl = flags;
    i = 0;
    while i < triangle_count {
        let mut j = 0u16;
        while j < 3 {
            ix[i * 3 + j] = attitude::get(input, cursor + j).lo_bits();
            j += 1;
        }
        fl[i] = attitude::get(input, cursor + 3).lo_bits();
        cursor += 4;
        i += 1;
    }
    let mut stride = count;
    i = 0;
    while i < 3 {
        let len = attitude::get(input, 41 + i);
        if len >= fix32::zero() && len.lo_bits() > stride {
            stride = len.lo_bits();
        }
        i += 1;
    }
    let raw_centers = at(flags, triangle_count);
    let mut centers = raw_centers;
    let radius = at(raw_centers, stride * 3);
    let gains = at(radius, 1);
    let mut ga = gains;
    i = 0;
    while i < 10 {
        ga[i] = attitude::get(input, 24 + i).lo_bits();
        i += 1;
    }
    let lengths = at(gains, 10);
    let mut le = lengths;
    i = 0;
    while i < 3 {
        le[i] = attitude::get(input, 41 + i).lo_bits();
        if le[i] != 65535 {
            let mut j = 0u16;
            while j < le[i] {
                centers[i * stride + j] = attitude::get(input, cursor).lo_bits();
                cursor += 1;
                j += 1;
            }
        }
        i += 1;
    }
    let mut r = radius;
    r[0u16] = attitude::get(input, 44).lo_bits();
    let state = mesh_break::State {
        header: scratch,
        faces: faces,
        count: count,
    };
    let mesh = mesh_break::Mesh {
        vertices: unsafe { raw_vertices.as_ptr().as_i16_array() },
        vertex_count: vertex_count,
        indices: indices,
        flags: flags,
        triangle_count: triangle_count,
        centers: unsafe { raw_centers.as_ptr().as_i16_array() },
        center_stride: stride,
        center_lengths: lengths,
        radius: radius,
    };
    let config = mesh_break::Config {
        gravity: attitude::get(input, 23),
        gains: gains,
    };
    FragmentCase {
        state: state,
        mesh: mesh,
        config: config,
    }
}
pub fn capture_mesh_break(
    out: Array<u16>,
    status: fix32,
    seed: fix32,
    state: &mesh_break::State,
    mesh: &mesh_break::Mesh,
) -> u16 {
    let scratch = state.header;
    let faces = state.faces;
    let count = state.count;
    let vertex_count = mesh.vertex_count;
    let triangle_count = mesh.triangle_count;
    let radius = mesh.radius;
    let flags = mesh.flags;
    let lengths = mesh.center_lengths;
    let stride = mesh.center_stride;
    let mut cursor = 0u16;
    let mut i = 0u16;
    attitude::put(out, 0, status);
    attitude::put(out, 1, seed);
    copy(at(out, 4), scratch, 8);
    attitude::put(out, 6, fix32::from_words(radius[0u16], 0));
    copy(at(out, 14), faces, count * 16);
    cursor = 7 + count * 8;
    i = 0;
    while i < vertex_count * 3 {
        attitude::put(out, cursor, integer::from_i16(mesh.vertices[i]));
        cursor += 1;
        i += 1;
    }
    i = 0;
    while i < triangle_count {
        attitude::put(out, cursor, fix32::from_words(flags[i], 0));
        cursor += 1;
        i += 1;
    }
    i = 0;
    while i < 3 {
        if lengths[i] != 65535 {
            let mut j = 0u16;
            while j < lengths[i] {
                attitude::put(out, cursor, integer::from_i16(mesh.centers[i * stride + j]));
                cursor += 1;
                j += 1;
            }
        }
        i += 1;
    }
    cursor * 2
}
pub fn run_mesh_break(
    input: Array<u16>,
    out: Array<u16>,
    scratch: Array<u16>,
    sine: Array<i16>,
) -> u16 {
    let data = load_mesh_break(input, scratch);
    let mut status = fix32::zero();
    let limit = attitude::get(input, 1);
    if attitude::get(input, 0) == fix32::zero() {
        let tail = attitude::get(input, 6);
        let hit = mesh_break::Hit {
            velocity: at(input, 16),
            has_velocity: attitude::get(input, 4) != fix32::zero(),
            basis: at(input, 22),
            has_basis: attitude::get(input, 5) != fix32::zero(),
            height: attitude::get(input, 7),
            paired_tail: tail.lo_bits(),
            has_paired_tail: tail >= fix32::zero(),
        };
        status = mesh_break::activate(
            &data.state,
            &data.mesh,
            &data.config,
            &hit,
            at(input, 6),
            limit,
        );
    } else {
        let step = mesh_break::Step {
            position: at(input, 40),
            dt: attitude::get(input, 2),
            limit: limit,
            sine: sine,
        };
        status = mesh_break::tick(&data.state, &data.mesh, &data.config, &step);
    }
    capture_mesh_break(
        out,
        status,
        attitude::get(input, 3),
        &data.state,
        &data.mesh,
    )
}
