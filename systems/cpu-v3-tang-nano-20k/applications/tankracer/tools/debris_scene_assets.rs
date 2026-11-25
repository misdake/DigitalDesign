//! Physical debris columns from constructed original objects.
use tankracer_game::{obstacle_startup::Debris, vehicle_runtime::SinglePlayer};

fn wide(out: &mut Vec<u16>, value: i32) {
    out.extend([value as u16, (value >> 16) as u16]);
}
fn columns(s: &Debris, hot: &mut Vec<u16>, info: &mut Vec<u16>) -> (u16, u16, u16) {
    let header = u16::try_from(hot.len()).unwrap();
    for value in [
        s.flags as i32,
        i32::from(s.retired),
        s.ground_height,
        s.gravity,
        s.friction,
        s.direction,
    ]
    .into_iter()
    .chain(s.offset)
    {
        wide(hot, value);
    }
    let pieces = u16::try_from(hot.len()).unwrap();
    for p in &s.pieces {
        for value in p
            .position
            .into_iter()
            .chain(p.velocity)
            .chain(p.matrix.0)
            .chain(p.angular)
            .chain([p.previous_tilt, p.flags as i32, p.sprite_angle])
        {
            wide(hot, value);
        }
        info.extend([
            p.velocity_gain as u16,
            p.spin_gain as u16,
            p.source.as_ref().map_or(0, |m| m.radius),
        ]);
    }
    let mut animation = 65535;
    if let Some(a) = &s.animation {
        animation = u16::try_from(hot.len()).unwrap();
        for value in a
            .position
            .into_iter()
            .chain([
                a.initial_size,
                i32::from(a.cycles),
                i32::from(a.growth),
                i32::from(a.timer),
                i32::from(a.rotation),
                i32::from(a.cycle),
            ])
            .chain(a.size)
            .chain(a.uv.map(i32::from))
            .chain([i32::from(a.sheet_size), i32::from(a.cell_size)])
        {
            wide(hot, value);
        }
    }
    (header, pieces, animation)
}
pub fn encode(world: &SinglePlayer) -> Vec<u16> {
    let mut directory = Vec::new();
    let mut info = Vec::new();
    let mut hot = Vec::new();
    for (id, object) in world.objects.components.objects.iter().enumerate() {
        let Some(s) = object.as_ref().and_then(|o| o.data.debris.as_ref()) else {
            continue;
        };
        let info_offset = u16::try_from(info.len()).unwrap();
        let (header, pieces, animation) = columns(s, &mut hot, &mut info);
        let radius = s.sprite.as_ref().map_or(0, |s| s.size[0]);
        directory.extend([
            u16::try_from(id).unwrap(),
            header,
            pieces,
            u16::try_from(s.pieces.len()).unwrap(),
            animation,
            info_offset,
            u16::from(s.sprite.is_some()),
            radius as u16,
            (radius >> 16) as u16,
            0,
        ]);
    }
    let count = directory.len() / 10;
    let info_offset = 16 + directory.len();
    let hot_offset = info_offset + info.len();
    let total = hot_offset + hot.len();
    let mut out = vec![
        0x4453,
        1,
        count as u16,
        hot.len() as u16,
        info.len() as u16,
        16,
        info_offset as u16,
        hot_offset as u16,
        total as u16,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    out.extend(directory);
    out.extend(info);
    out.extend(hot);
    assert_eq!(out.len(), total);
    out
}
pub fn bindings(world: &SinglePlayer) -> Vec<u16> {
    let mut out = tankracer_game::symbols::rcc_probe::encode(&world.identity.symbols);
    for state in world.objects.components.falls.iter().flatten() {
        if let Some(attached) = &state.attached {
            for binding in attached.iter() {
                out.extend(tankracer_game::symbols::rcc_probe::binding(*binding));
            }
        }
    }
    out
}
pub fn cars(world: &SinglePlayer) -> Vec<u16> {
    let mut out = Vec::new();
    for p in world.vehicles.entries[..2].iter().flatten() {
        let v = &p.physics;
        wide(&mut out, v.entity.0 as i32);
        out.extend([u16::from(p.slot), u16::from(v.player), u16::from(p.visible)]);
        for value in [
            v.blast.health,
            i32::from(v.blast.health_timer),
            i32::from(v.blast.health_color),
            v.blast.protection_timer,
            i32::from(v.blast.angular_enabled),
            i32::from(v.blast.hit_axes[0]),
            i32::from(v.blast.hit_axes[1]),
        ] {
            wide(&mut out, value);
        }
    }
    assert_eq!(out.len(), 38);
    out
}
// Frame snapshots use precisely the loader's physical columns. Identity and
// cold resource references are checked separately by their owning qualifiers.
pub fn capture(world: &SinglePlayer) -> Vec<u16> {
    let mut out = vec![0];
    for (id, object) in world.objects.components.objects.iter().enumerate() {
        if ![169, 170, 171, 179, 180, 181].contains(&id) {
            continue;
        }
        let s = object.as_ref().and_then(|o| o.data.debris.as_ref());
        out.extend([id as u16, u16::from(s.is_some())]);
        if let Some(s) = s {
            let mut hot = Vec::new();
            let mut info = Vec::new();
            columns(s, &mut hot, &mut info);
            out.extend(hot);
            out.extend(info);
        }
    }
    out.extend(cars(world));
    out.extend(tankracer_game::symbols::rcc_probe::encode(
        &world.identity.symbols,
    ));
    out[0] = u16::try_from(out.len()).unwrap();
    out
}
