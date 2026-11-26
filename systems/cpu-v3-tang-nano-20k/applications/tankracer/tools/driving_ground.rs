//! Original synchronous ground response and effects on persistent driving state.
use tankracer_assets::mesh::Geometry;
use tankracer_game::{
    contact_pipeline, damage, effect_pool, ground_collision, impact_particles, motion,
    object_query::EntityId, powertrain, vehicle,
};

#[path = "impact_snapshot.rs"]
mod capture;

pub struct State {
    vehicle: vehicle::State,
    pub damage: damage::State,
    pub mesh: Geometry,
    selected: [u8; 4],
    impacts: impact_particles::State,
    pool: effect_pool::Pool,
    pub delta: [i32; 3],
}

struct Effects<'a> {
    vehicle: &'a mut vehicle::State,
    damage: damage::Target<'a>,
    impacts: &'a mut impact_particles::State,
    context: impact_particles::Context<'a>,
    corners: [u32; 4],
}
impl ground_collision::Effects for Effects<'_> {
    fn impact(&mut self, contact: usize, kind: i32) -> Result<(), i32> {
        self.impacts
            .emit(self.vehicle, self.corners[contact], kind, &mut self.context)
            .map(|_| ())
            .map_err(impact_particles::Error::status)
    }
    fn track_damage(&mut self, kind: i32, world: [i32; 3]) -> Result<(), i32> {
        self.damage.track(kind, world).map_err(|_| -1)
    }
}
impl State {
    // Raw trajectories execute the visible physical branch directly. Export
    // its actual vehicle fields instead of relying on empty scene bindings.
    pub fn bindings(&self) -> Vec<u16> {
        let v = &self.vehicle;
        let mut out = vec![
            v.entity.0 as u16,
            (v.entity.0 >> 16) as u16,
            0,
            u16::from(v.player),
            1,
        ];
        for value in [
            v.blast.health,
            i32::from(v.blast.health_timer),
            i32::from(v.blast.health_color),
            v.blast.protection_timer,
            i32::from(v.blast.angular_enabled),
            i32::from(v.blast.hit_axes[0]),
            i32::from(v.blast.hit_axes[1]),
        ] {
            out.extend([value as u16, (value >> 16) as u16]);
        }
        out
    }
    pub fn new(mesh: Geometry, selected: [u8; 4]) -> Self {
        Self {
            vehicle: vehicle::State {
                entity: EntityId(1),
                pose_input: motion::PoseInput {
                    external: tankracer_game::pose::ExternalBasis::Body,
                    ..Default::default()
                },
                ..Default::default()
            },
            damage: damage::State::default(),
            mesh,
            selected,
            impacts: impact_particles::State::default(),
            pool: effect_pool::Pool::default(),
            delta: [0; 3],
        }
    }
    pub fn respond(
        &mut self,
        m: &mut motion::State,
        p: &powertrain::State,
        g: &mut contact_pipeline::Pipeline,
        number: u32,
        seed: &mut u32,
    ) {
        self.vehicle.motion = m.clone();
        self.vehicle.powertrain = *p;
        let mut contacts = std::array::from_fn(|i| ground_collision::Contact {
            kind: g.sampling.primary.points[i].kind as i16,
            corner: g.response.points[i].corner as i32,
            normal: g.sampling.primary.points[i].normal,
        });
        let initial = m.velocity;
        let mut state = ground_collision::State {
            velocity: initial,
            yaw: m.angular[1],
        };
        state
            .step(
                &mut contacts,
                ground_collision::Environment {
                    mode: 0,
                    player: true,
                    forward_velocity: p.local_velocity[2],
                    position: m.poses.base_position,
                    external_basis: self.vehicle.external_basis(),
                },
                &mut Effects {
                    vehicle: &mut self.vehicle,
                    damage: damage::Target {
                        state: &mut self.damage,
                        player: true,
                        body_pose: m.poses.body,
                        body_vertices: self.selected,
                        body_mesh: Some(&mut self.mesh),
                    },
                    impacts: &mut self.impacts,
                    context: impact_particles::Context {
                        frame: number,
                        random: seed,
                        pool: &mut self.pool,
                        tuning: impact_particles::Tuning::default(),
                    },
                    corners: g.response.points.map(|p| p.corner),
                },
            )
            .unwrap();
        m.velocity = state.velocity;
        m.angular[1] = state.yaw;
        self.delta = std::array::from_fn(|i| state.velocity[i].wrapping_sub(initial[i]));
        for (point, contact) in g.sampling.primary.points.iter_mut().zip(contacts) {
            point.normal = contact.normal;
        }
    }
    pub fn tick(
        &mut self,
        m: &motion::State,
        p: &powertrain::State,
        number: u32,
        dt: i32,
        seed: &mut u32,
    ) {
        self.vehicle.motion = m.clone();
        self.vehicle.powertrain = *p;
        self.impacts
            .tick(
                &mut self.vehicle,
                dt,
                &mut impact_particles::Context {
                    frame: number,
                    random: seed,
                    pool: &mut self.pool,
                    tuning: impact_particles::Tuning::default(),
                },
            )
            .unwrap();
    }
    pub fn snapshot(&self, seed: u32) -> Vec<i32> {
        let mut out = capture::snapshot([0; 4], &self.impacts, &self.vehicle, &self.pool, seed);
        out.extend(self.damage.body.map(i32::from));
        out.extend(self.damage.turret.map(i32::from));
        out.extend(self.damage.tracks.map(i32::from));
        out.extend(self.delta);
        out.extend(self.mesh.vertices.iter().flatten().copied().map(i32::from));
        out.extend(
            self.mesh
                .triangles
                .iter()
                .flat_map(|t| t.normal.map(i32::from)),
        );
        out
    }
}
