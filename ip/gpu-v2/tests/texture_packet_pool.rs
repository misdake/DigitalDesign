#[path = "support/sdram/physical_texture.rs"]
mod physical;
#[path = "support/texture.rs"]
#[allow(dead_code)]
mod support;
use gpu_v2::texture::{
    ports::*,
    sim::{oracle, staged::bound, timed},
};
use support::*;
#[test]
fn actual_pool_connection_handles_slots_masks_ce_and_result_recovery() {
    let a = slot(5, true);
    let mut b = slot(5, false);
    let mut bytes = asset(a, pattern);
    b.base_address = BASE + bytes.len() as u32;
    bytes.extend(asset(b, |n, x, y| pattern(n, x, y) ^ u16::MAX));
    let slots = [a, b];
    let qs: Vec<_> = (0..24)
        .map(|i| {
            let mut q = input(
                5,
                [Filter::Nearest, Filter::Bilinear, Filter::Trilinear][i % 3],
                [0.13, 0.07],
            );
            q.quad_id = (i % 4) as u8;
            // The same quad ID changes texture slot on every reuse. Distinct
            // texels in the two assets make a stale binding observable in RGB.
            q.slot = ((i / 4) % 2) as u8;
            q.mask = [15, 1, 6, 0][i % 4];
            q.uv[1][0] += 1.0 / 32.0;
            q.uv[2][1] += 1.0 / 32.0;
            q.uv[3][0] += 1.0 / 32.0;
            q.uv[3][1] += 1.0 / 32.0;
            q.lod_bias = 0.5;
            q
        })
        .collect();
    let mut image = Image {
        bytes: bytes.clone(),
        requests: vec![],
    };
    let mut oc = oracle::Cache::new(slots.to_vec()).unwrap();
    let expected: Vec<_> = qs
        .iter()
        .flat_map(|q| {
            oracle::sample(q, &mut oc, &mut image, Config::counted())
                .unwrap()
                .pixels
                .into_iter()
                .map(|p| timed::PixelResult {
                    quad_id: q.quad_id,
                    lane: p.lane,
                    rgb: p.rgb,
                })
        })
        .collect();
    let mut memory = physical::Physical::new(u64::from(BASE), bytes, true, false);
    let mut r = bound::system::run_pooled(
        &qs,
        &slots,
        &mut memory,
        bound::control::Hardware {
            storage: bound::control::Storage::Packed,
            ..Default::default()
        },
        timed::Hardware {
            prefetch: false,
            max_cycles: 100_000,
            ..Default::default()
        },
        |t| timed::Control {
            ce: t % 23 > 7,
            result_ready: t > 600 && t % 31 > 7,
        },
    )
    .unwrap();
    assert_eq!(r.cache.pixels, expected);
    let mut active = [None; 16];
    let mut prep_released = [false; 16];
    let mut accepted = 0;
    let mut read_after_prep_release = false;
    for (prep, cache) in r.preparation.iter().zip(&r.cache.steps) {
        for event in &cache.events {
            match event {
                timed::Event::Accepted { quad, .. } => {
                    let q = &qs[accepted];
                    assert_eq!(*quad, q.quad_id);
                    assert!(active[usize::from(*quad)].is_none());
                    active[usize::from(*quad)] = Some(q.slot);
                    prep_released[usize::from(*quad)] = false;
                    accepted += 1;
                }
                timed::Event::Read { group, .. } => {
                    let id = usize::from(group.quad_id);
                    assert_eq!(Some(group.key.slot), active[id]);
                    read_after_prep_release |= prep_released[id];
                }
                _ => {}
            }
        }
        for event in &prep.events {
            if let bound::control::Event::Release { program } = event {
                prep_released[usize::from(qs[*program].quad_id)] = true;
            }
        }
        for (id, binding) in active.iter_mut().enumerate() {
            if cache.snapshot.live_quads >> id & 1 == 0 {
                *binding = None;
            }
        }
    }
    assert_eq!(accepted, qs.len());
    assert!(
        read_after_prep_release,
        "slot must survive preparation release"
    );
    assert!(r
        .cache
        .steps
        .iter()
        .any(|s| !s.control.ce && !s.responses.is_empty()));
    assert!(r.cache.stats.peak_results > 0);
    assert!(!r
        .cache
        .steps
        .iter()
        .take_while(|s| s.cycle <= 600)
        .flat_map(|s| &s.events)
        .any(|e| matches!(e, timed::Event::Commit { .. })));
    assert!(memory.init_cycles > 0);
    let index = r
        .cache
        .steps
        .iter()
        .position(|s| {
            s.packet_events
                .iter()
                .any(|e| matches!(e, timed::packet::Event::Capture { .. }))
        })
        .unwrap();
    let capture = r.cache.steps[index]
        .packet_events
        .iter_mut()
        .find(|e| matches!(e, timed::packet::Event::Capture { .. }))
        .unwrap();
    if let timed::packet::Event::Capture { payload, .. } = capture {
        *payload ^= 1;
    }
    assert!(
        r.audit().unwrap_err().contains("controller replay differs"),
        "corrupted capture trace must be detected by replay"
    );
}
#[test]
fn pooled_inventory_replaces_payload_records_instead_of_stacking_them() {
    let b = bound::Binding::build().unwrap();
    let p = bound::control::Hardware {
        storage: bound::control::Storage::Packed,
        ..Default::default()
    };
    let native = timed::Hardware {
        preparation: timed::PreparationMode::BoundStages,
        prefetch: false,
        ..Default::default()
    };
    let old = bound::inventory::describe(&b, p, &native).unwrap();
    let pooled = timed::Hardware {
        packet_storage: timed::PacketStorage::Pool64,
        ..native
    };
    let new = bound::inventory::describe(&b, p, &pooled).unwrap();
    assert!(!new
        .rows
        .iter()
        .any(|r| matches!(r.name, "Group4 FIFO" | "packet holding records")));
    assert_eq!(new.bsram, old.bsram + 2);
    assert_eq!(new.sdp4_cells + 36, old.sdp4_cells);
    assert_eq!(new.ff_bits + 508, old.ff_bits);
    assert_eq!(new.hard_pipeline_bits, old.hard_pipeline_bits);
}
