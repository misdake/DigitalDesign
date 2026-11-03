use super::*;

pub(super) fn stage_isolation(
    path: &str,
    memory: &Memory<'_>,
    draws: &[DrawInput],
    setups: &[Result<Setup, String>],
    strips: &[Strip],
    fused: &[Result<Raster, String>],
) {
    use std::io::Write;
    struct Unshaded {
        xy: [usize; 2],
        fragment: Fragment,
        lod: u8,
        draw: usize,
    }
    let mut output = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    for threads in [1, 2, 4, 8] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        for repeat in 0..4 {
            let start = Instant::now();
            let geometry: Vec<Vec<Unshaded>> = pool.install(|| {
                strips
                    .par_iter()
                    .map(|strip| {
                        let setup = setups[strip.triangle].as_ref().unwrap();
                        let triangle = setup.triangle.as_ref().unwrap();
                        let state = draws[setup.draw].state;
                        let mut output = Vec::new();
                        for row in strip.first..strip.end {
                            let spans = setup.spans[row];
                            if let Some(range) = span_range(spans) {
                                for x in range.step_by(2) {
                                    let quad = Quad::new(x, row * 2, spans);
                                    let fragments = quad
                                        .fragments(
                                            triangle,
                                            state.global.mapped_far(),
                                            state.global.depth_shift,
                                        )
                                        .unwrap();
                                    let lod =
                                        fragments.iter().flatten().next().map_or(0, |first| {
                                            if state.rgb {
                                                0
                                            } else {
                                                triangle.lod(first, state.render)
                                            }
                                        });
                                    for (lane, fragment) in fragments.into_iter().enumerate() {
                                        if let Some(fragment) = fragment.filter(|f| f.valid) {
                                            output.push(Unshaded {
                                                xy: quad.xy(lane),
                                                fragment,
                                                lod,
                                                draw: setup.draw,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        output
                    })
                    .collect()
            });
            let geometry_ns = ns(start);
            let start = Instant::now();
            let shaded: Vec<Vec<Sample>> = pool.install(|| {
                geometry
                    .par_iter()
                    .map(|strip| {
                        strip
                            .iter()
                            .map(|sample| {
                                let mut report = Report::default();
                                let color = shading::shade(
                                    memory,
                                    sample.xy,
                                    &sample.fragment,
                                    &draws[sample.draw].state,
                                    sample.lod,
                                    &mut report,
                                    usize::MAX,
                                )
                                .unwrap();
                                Sample {
                                    x: sample.xy[0] as u16,
                                    y: sample.xy[1] as u16,
                                    depth: sample.fragment.depth,
                                    draw: sample.draw as u16,
                                    color,
                                }
                            })
                            .collect()
                    })
                    .collect()
            });
            let shade_ns = ns(start);
            for (index, strip) in shaded.iter().enumerate() {
                assert_eq!(*strip, fused[index].as_ref().unwrap().pixels);
            }
            writeln!(output,"{{\"threads\":{threads},\"repeat\":{repeat},\"geometry_ns\":{geometry_ns},\"shade_ns\":{shade_ns},\"samples\":{},\"exact_fused_samples_equal\":true,\"method\":\"separate materialized diagnostic; not an additive partition of fused raster_ns\"}}",shaded.iter().map(Vec::len).sum::<usize>()).unwrap();
        }
    }
}

#[test]
fn empty_draw_events_and_later_pixel_free_state_are_both_preserved() {
    let mut state = State::default();
    let target = [0x20000, 0x40000];
    state.target = Some(target);
    state.loaded.fill(true);
    state.uniforms.fill(true);
    let mut global = [0; 64];
    for (i, value) in [
        100u32 * 65536,
        0,
        16384 | (8192 << 16),
        4915 | (10486 << 16),
        256,
        0,
    ]
    .into_iter()
    .enumerate()
    {
        global[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    state.global = Some(shading::Global::decode(global).unwrap());
    state.render = Some(shading::RenderState::decode([0; 32]).unwrap());
    let mut uniform = [0u32; 32];
    for i in [0, 5, 10, 12, 16, 20, 21] {
        uniform[i] = 65536;
    }
    uniform[22] = 0xffffffff;
    for (i, word) in uniform.into_iter().enumerate() {
        state.local[1280 + i * 2] = word as u16;
        state.local[1280 + i * 2 + 1] = (word >> 16) as u16;
    }
    // All z are negative: DRAW bindings remain real events without any Pixels.
    state.pages[0] = Some(Meshlet {
        vertices: vec![
            [0, 0, 255],
            [512, 0, 255 << 8],
            [256 | (256 << 10), 0, 255 << 16],
        ],
        triangles: vec![[0, 1, 2]],
        origin: [-65536, -32768, -4 * 65536],
        shift: 8,
        normal18: false,
        uniform: 0,
        rgb: true,
        cull: false,
    });
    state.local[66 * 16 + 1] = 4; // Later Render changes texture size to4.
    global[..4].copy_from_slice(&(50u32 * 65536).to_le_bytes());
    for (i, word) in global.as_chunks::<2>().0.iter().enumerate() {
        state.local[68 * 16 + i] = u16::from_le_bytes(*word);
    }
    let commands = [
        0x30000000u32,
        0x30000000,
        0x40000003,
        0x70000000,
        0x60000042,
        0x50000044,
        0xf0000000,
    ];
    let mut original = vec![0u16; 0x400000];
    original[target[1]..target[1] + PLANE_WORDS].fill(65535);
    for (i, word) in commands.into_iter().enumerate() {
        original[i * 2] = word as u16;
        original[i * 2 + 1] = (word >> 16) as u16;
    }
    let mut reference_words = original.clone();
    let mut reference = Memory {
        words: &mut reference_words,
        access: None,
        effects: Effects::default(),
    };
    let mut reference_state = state.clone();
    for word in &commands[..6] {
        assert!(!reference_state
            .command(&mut reference, *word, 64_000_000)
            .unwrap());
    }
    let mut actual = Memory {
        words: &mut original,
        access: None,
        effects: Effects::default(),
    };
    let mut stats = BatchStats::default();
    let Attempt::Applied(next, pc) = try_execute(
        &mut actual,
        &state,
        0,
        0..7,
        64_000_000,
        RebinMode::TileScan,
        &mut stats,
    ) else {
        panic!("empty draws must still execute the enum batch");
    };
    assert_eq!(pc, 6);
    assert_eq!(next.report, reference_state.report);
    assert_eq!(next.global.unwrap().far, 50 * 65536);
    assert_eq!(next.render.unwrap().texture.size_log2, 4);
    assert_eq!(next.local, reference_state.local);
    assert_eq!(next.all_fence, reference_state.all_fence);
    assert_eq!(next.stored, reference_state.stored);
    assert_eq!(actual.effects, reference.effects);
    assert!(actual.words == reference.words);
    assert_eq!(stats.messages, [2, 0, 2, 800]);
}
pub(super) fn compare_shared_rebin(
    path: &str,
    source: &[TileMessage],
    groups: &[std::ops::Range<usize>],
    pieces: &[Piece],
) {
    use std::io::Write;
    let reference = rebin(source, groups, pieces, RebinMode::TileScan).bins;
    let mut output = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    for threads in [1, 2, 4, 8] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        for (name, mode) in [
            ("A", RebinMode::TriangleTiles),
            ("B", RebinMode::TileScan),
            ("C", RebinMode::ParallelSort),
            ("D", RebinMode::LocalBuckets),
        ] {
            for repeat in 0..4 {
                let result = pool.install(|| rebin(source, groups, pieces, mode));
                assert_eq!(
                    result.bins, reference,
                    "shared prepared event input: {threads} {name}"
                );
                writeln!(output,"{{\"threads\":{threads},\"mode\":\"{name}\",\"repeat\":{repeat},\"events\":{},\"pieces\":{},\"groups\":{},\"samples\":{},\"rebin_ns\":{},\"sort_ns\":{},\"extra_payload_peak\":{},\"exact_stream_equal\":true}}",
                    source.len(),pieces.len(),groups.len(),pieces.iter().map(|p|p.pixels.len()).sum::<usize>(),
                    result.rebin_ns,result.sort_ns,result.peak_bytes).unwrap();
            }
        }
    }
}

#[test]
#[ignore = "bounded same-prepared-input rebin comparison; requires explicit frozen files/output"]
fn shared_prepared_rebin_comparison() {
    assert!(!std::path::Path::new(&std::env::var_os("GPU_SIM_SHARED_REBIN_OUT").unwrap()).exists());
    execute_frozen_fixture();
}

#[test]
#[ignore = "bounded separate geometry/shade diagnostic on explicit frozen files"]
fn isolated_stage_diagnostic() {
    assert!(
        !std::path::Path::new(&std::env::var_os("GPU_SIM_STAGE_DIAGNOSTIC_OUT").unwrap()).exists()
    );
    execute_frozen_fixture();
}

fn execute_frozen_fixture() {
    let path = std::path::PathBuf::from(std::env::var_os("GPU_SIM_SHARED_INPUT").unwrap());
    let bytes = std::fs::read(path.join("memory-before.bin")).unwrap();
    let expected = std::fs::read(path.join("memory-after.bin")).unwrap();
    let mut words: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let mut flags = std::fs::read(path.join("access-before.bin")).unwrap();
    let count = std::env::var("GPU_SIM_SHARED_COUNT")
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let submission =
        crate::system::vliw_command::DeviceSubmission::new(4980736, (count + 1) * 4).unwrap();
    let mut executor = crate::system::functional::Executor::new(64_000_000);
    executor
        .execute_captured(&mut words, &mut flags, submission)
        .unwrap();
    assert!(executor.batch_stats().applied > 0);
    assert!(words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .eq(expected));
}
