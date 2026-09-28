use super::*;

fn vertex(x: i32, y: i32, z: i32) -> RawVertex {
    RawVertex {
        position: [x, y, z, 0x1_0000],
        rgb565: 0xf800,
    }
}

#[test]
fn mvp_rounds_signed_half_ties_to_even() {
    let mut matrix = [[0; 4]; 4];
    matrix[0][0] = 1;
    assert_eq!(transform([32_768, 0, 0, 0], &matrix).unwrap()[0], 0);
    assert_eq!(transform([98_304, 0, 0, 0], &matrix).unwrap()[0], 2);
    assert_eq!(transform([-32_768, 0, 0, 0], &matrix).unwrap()[0], 0);
    assert_eq!(transform([-98_304, 0, 0, 0], &matrix).unwrap()[0], -2);
}

fn fixture() -> Meshlet {
    let mut matrix = [[0; 4]; 4];
    for (axis, row) in matrix.iter_mut().enumerate() {
        row[axis] = 0x1_0000;
    }
    Meshlet {
        matrix,
        vertices: vec![
            vertex(-0x199a, -0x199a, 0x8000),
            vertex(0x199a, -0x199a, 0x8000),
            vertex(0, 0x199a, 0x8000),
            vertex(-0x199a, -0x199a, -0x8000),
            vertex(0x199a, -0x199a, -0x8000),
            vertex(0, 0x199a, -0x8000),
        ],
        triangles: vec![[0, 2, 1], [3, 2, 1], [3, 4, 5], [0, 1, 63]],
    }
}

#[test]
fn raw_slot_round_trip_and_source_semantics() {
    let meshlet = fixture();
    let words = meshlet.words().unwrap();
    assert_eq!(words[0], 0x0000_0000_0001_0000);
    assert_eq!(words[TRIANGLE_BASE], 0x0001_0200);
    let decoded =
        Meshlet::from_words(&words, meshlet.vertices.len(), meshlet.triangles.len()).unwrap();
    assert_eq!(decoded, meshlet);

    let records = run_meshlet(1, &meshlet, 20_000).unwrap();
    let mut fans = [0; 4];
    let mut primitive_ends = [0; 4];
    let mut ends = Vec::new();
    let mut quads = 0;
    for record in &records {
        match record {
            SimRecord::Fan { slot, source, .. } => {
                assert_eq!(*slot, 1);
                fans[usize::from(*source)] += 1;
            }
            SimRecord::Quad {
                slot, mask, color, ..
            } => {
                assert_eq!(*slot, 1);
                assert_ne!(*mask, 0);
                for (lane, code) in color.iter().enumerate() {
                    if mask & (1 << lane) != 0 {
                        assert_eq!(*code, 0xf800);
                    }
                }
                quads += 1;
            }
            SimRecord::SourceEnd {
                source,
                last,
                fault,
                ..
            } => ends.push((*source, *last, *fault)),
            SimRecord::TileEnd { slot, .. } => assert_eq!(*slot, 1),
            SimRecord::PrimitiveEnd { slot, source, .. } => {
                assert_eq!(*slot, 1);
                primitive_ends[usize::from(*source)] += 1;
            }
        }
    }
    assert_eq!(fans, [1, 2, 0, 0]);
    assert_eq!(primitive_ends, fans);
    assert!(quads > 0);
    assert_eq!(ends.len(), 4);
    assert_eq!(ends[0], (0, false, None));
    assert_eq!(ends[1], (1, false, None));
    assert_eq!(ends[2], (2, false, None));
    assert_eq!(
        ends[3],
        (
            3,
            true,
            Some(SourceFault::Index {
                corner: 2,
                index: 63
            })
        )
    );
    assert_eq!(
        run_meshlet(1, &meshlet, 1),
        Err(SimError::RecordLimit { max_records: 1 })
    );
}

#[test]
fn source_end_follows_every_fan_and_rejected_source() {
    let meshlet = fixture();
    let records = run_meshlet(0, &meshlet, 20_000).unwrap();
    for source in 0..meshlet.triangles.len() {
        let source_records: Vec<_> = records
            .iter()
            .filter(|record| match record {
                SimRecord::Fan { source: id, .. }
                | SimRecord::Quad { source: id, .. }
                | SimRecord::TileEnd { source: id, .. }
                | SimRecord::PrimitiveEnd { source: id, .. }
                | SimRecord::SourceEnd { source: id, .. } => usize::from(*id) == source,
            })
            .collect();
        assert!(matches!(
            source_records.last(),
            Some(SimRecord::SourceEnd { .. })
        ));
        for (primitive, record) in source_records.iter().enumerate() {
            if let SimRecord::Fan { primitive: fan, .. } = record {
                assert!(source_records[primitive + 1..].iter().any(|later| {
                    matches!(later, SimRecord::PrimitiveEnd { primitive: end, .. } if end == fan)
                }));
            }
        }
    }
}

#[test]
fn mvp_overflow_retires_without_clipping() {
    let mut meshlet = fixture();
    meshlet.triangles.truncate(1);
    meshlet.triangles[0] = [0, 2, 1];
    meshlet.matrix[0][0] = 0x2_0000;
    meshlet.vertices[0].position[0] = 0x6000_0000;
    assert_eq!(
        run_meshlet(0, &meshlet, 100).unwrap(),
        vec![SimRecord::SourceEnd {
            slot: 0,
            source: 0,
            last: true,
            fault: Some(SourceFault::MvpOverflow { corner: 0, row: 0 }),
        }]
    );
}

#[test]
fn empty_meshlet_and_capacity_are_explicit() {
    let mut meshlet = fixture();
    meshlet.triangles.clear();
    assert!(run_meshlet(0, &meshlet, 1).unwrap().is_empty());
    meshlet.vertices.resize(65, vertex(0, 0, 0));
    assert_eq!(meshlet.words(), Err(SimError::Capacity));
}
