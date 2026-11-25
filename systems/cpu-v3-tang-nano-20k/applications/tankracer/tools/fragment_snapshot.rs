use tankracer_assets::mesh::Geometry;
use tankracer_game::{
    math::Matrix, mesh_break, obstacle_startup::Auxiliary, scene_meshes::FragmentMesh,
};
#[derive(Clone)]
pub struct Fixture {
    pub state: mesh_break::State,
    pub geometry: Geometry,
    pub centers: [Option<Vec<i16>>; 3],
    pub radius: u16,
    pub config: Auxiliary,
    pub seed: u32,
    pub velocity: Option<[i32; 3]>,
    pub basis: Option<Matrix>,
    pub height: i32,
    pub position: [i32; 3],
    pub tail: Option<u16>,
}
fn face(out: &mut Vec<i32>, face: &mesh_break::Face) {
    out.extend(face.velocity);
    out.extend(face.angular);
    out.extend([
        face.life,
        i32::from(face.extra_vertex as u16) | (i32::from(face.mode) << 16),
    ]);
}
impl Fixture {
    pub fn input(&self, op: i32, limit: u32, dt: i32) -> Vec<i32> {
        let mut out = vec![
            op,
            limit as i32,
            dt,
            self.seed as i32,
            i32::from(self.velocity.is_some()),
            i32::from(self.basis.is_some()),
            self.tail.map_or(-1, i32::from),
            self.height,
        ];
        out.extend(self.velocity.unwrap_or([0; 3]));
        out.extend(self.basis.unwrap_or(Matrix::IDENTITY).0);
        out.extend(self.position);
        out.push(self.config.gravity);
        for gain in [
            self.config.bounce,
            self.config.friction,
            self.config.push,
            self.config.velocity,
            self.config.spin,
        ] {
            out.extend([i32::from(gain.multiplier), i32::from(gain.shift)]);
        }
        out.extend([
            self.state.retired as i32,
            self.state.ground_height,
            i32::from(self.state.prepared),
            self.state.lifetime,
            self.state.faces.len() as i32,
            self.geometry.vertices.len() as i32,
            self.geometry.triangles.len() as i32,
        ]);
        out.extend(
            self.centers
                .iter()
                .map(|v| v.as_ref().map_or(-1, |v| v.len() as i32)),
        );
        out.push(i32::from(self.radius));
        assert_eq!(out.len(), 45);
        for f in &self.state.faces {
            face(&mut out, f);
        }
        out.extend(
            self.geometry
                .vertices
                .iter()
                .flatten()
                .map(|v| i32::from(*v)),
        );
        for tri in &self.geometry.triangles {
            out.extend(tri.indices.map(i32::from));
            out.push(i32::from(tri.flags));
        }
        for c in self.centers.iter().flatten() {
            out.extend(c.iter().map(|v| i32::from(*v)));
        }
        out
    }
    pub fn output(&self, status: i32) -> Vec<i32> {
        let mut out = vec![
            status,
            self.seed as i32,
            self.state.retired as i32,
            self.state.ground_height,
            i32::from(self.state.prepared),
            self.state.lifetime,
            i32::from(self.radius),
        ];
        for f in &self.state.faces {
            face(&mut out, f);
        }
        out.extend(
            self.geometry
                .vertices
                .iter()
                .flatten()
                .map(|v| i32::from(*v)),
        );
        out.extend(self.geometry.triangles.iter().map(|t| i32::from(t.flags)));
        for c in self.centers.iter().flatten() {
            out.extend(c.iter().map(|v| i32::from(*v)));
        }
        out
    }
    pub fn run(&mut self, op: i32, limit: u32, dt: i32) -> i32 {
        let mesh = FragmentMesh {
            geometry: &mut self.geometry,
            centers: &mut self.centers,
            radius: &mut self.radius,
        };
        if op == 0 {
            mesh_break::activate(
                &mut self.state,
                mesh,
                &self.config,
                mesh_break::Hit {
                    velocity: self.velocity,
                    basis: self.basis,
                    height: self.height,
                    paired_tail: self.tail,
                },
                &mut self.seed,
                limit,
            )
            .err()
            .unwrap_or(0)
        } else {
            mesh_break::tick(
                &mut self.state,
                mesh,
                &self.config,
                self.position,
                dt,
                limit,
            )
            .map(i32::from)
            .unwrap_or_else(|e| e)
        }
    }
}
