//! Optional complete-trace diagnostic; the frozen geometry study is not a
//! production integration dependency and must not be silently merged with it.
use gpu_v2_geometry_study::{calendar, candidate::ClipMethod, corpus, counted};
use gpu_v2_scaled_output_binding::binding;

fn main() {
    println!("case,method,planes,expressions,base_cycles,bound_cycles,base_issues,bound_issues,base_peak_bits,bound_peak_bits,base_bit_cycles,bound_bit_cycles,base_dependency_bits,bound_dependency_bits");
    let mut paths = 0;
    let mut formats = std::collections::BTreeSet::new();
    for (name, input) in corpus::cases() {
        for method in [ClipMethod::Attributes, ClipMethod::Weights] {
            for planes in [
                counted::Planes::Wide,
                counted::Planes::Conservative,
                counted::Planes::MixedRgb,
            ] {
                let report = counted::run(
                    &input,
                    counted::Settings {
                        method,
                        planes,
                        ..Default::default()
                    },
                )
                .expect("finite geometry numerical kernel");
                let original =
                    calendar::schedule_style(&report.frame, 2, true).expect("six-DSP calendar");
                for group in binding::groups(&report.frame).unwrap() {
                    formats.insert((
                        group.spec.fraction,
                        group.spec.bits,
                        group.spec.out_fraction,
                    ));
                }
                let result = binding::compare(&report.frame, &original.graph)
                    .expect("closed binding and calendar");
                let a = &result.baseline;
                let b = &result.candidate;
                assert_eq!(result.graph.resources, original.graph.resources);
                assert_eq!(a.cycles, original.schedule.cycles);
                assert_eq!(a.peak_payload_bits, original.peak_live_bits);
                println!(
                    "{name},{method:?},{planes:?},{},{},{},{},{},{},{},{},{},{},{}",
                    result.expressions,
                    a.cycles,
                    b.cycles,
                    a.resource_issues,
                    b.resource_issues,
                    a.peak_payload_bits,
                    b.peak_payload_bits,
                    a.payload_bit_cycles,
                    b.payload_bit_cycles,
                    a.dependency_payload_bits,
                    b.dependency_payload_bits
                );
                eprintln!(
                    "{name}/{method:?}/{planes:?}: base_roles={:?}; bound_roles={:?}",
                    a.issues_by_role, b.issues_by_role
                );
                paths += 1;
                assert!(paths <= 36, "bounded representative traces");
            }
        }
    }
    assert_eq!(paths, 36);
    assert_eq!(formats.len(), 8, "all actual output formats");
}
