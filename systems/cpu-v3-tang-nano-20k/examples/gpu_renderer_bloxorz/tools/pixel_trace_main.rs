//! Entry for generated, observation-only functional GPU copies.
#![allow(dead_code)]
pub use gpu_v2::framebuffer;
mod frontend {
    include!("numeric.rs");
    pub(crate) use words as interpolation_constants;
}
mod lighting {
    pub use gpu_v2::lighting::ports;
    include!("lighting_constants.rs");
}
mod vliw_command {
    pub use gpu_v2::system::vliw_command::*;
}
mod functional;
mod trace {
    use serde_json::{json, Value};
    use std::{
        cell::RefCell,
        sync::{Mutex, OnceLock},
    };
    static POINTS: OnceLock<Vec<[usize; 2]>> = OnceLock::new();
    static CONTEXT: Mutex<[usize; 4]> = Mutex::new([0; 4]);
    thread_local! {static TAPS:RefCell<Option<Vec<Value>>>=const{RefCell::new(None)};}
    pub fn configure(points: Vec<[usize; 2]>) {
        POINTS.set(points).unwrap();
    }
    pub fn points() -> Vec<[usize; 2]> {
        POINTS.get().unwrap().clone()
    }
    pub fn submission(i: usize) {
        *CONTEXT.lock().unwrap() = [i, 0, 0, 0];
    }
    pub fn command(pc: usize, word: u32) {
        let mut c = CONTEXT.lock().unwrap();
        c[1] = pc;
        c[2] = word as usize;
        if word >> 28 == 3 {
            c[3] += 1;
        }
    }
    pub fn context() -> Value {
        let c = *CONTEXT.lock().unwrap();
        json!({"submission":c[0],"pc_u32":c[1],"command":c[2],"draw_1based":c[3]})
    }
    pub fn begin_taps() {
        TAPS.with(|t| *t.borrow_mut() = Some(Vec::new()));
    }
    pub fn tap(v: Value) {
        TAPS.with(|t| {
            if let Some(out) = t.borrow_mut().as_mut() {
                out.push(v);
            }
        });
    }
    pub fn end_taps() -> Vec<Value> {
        TAPS.with(|t| t.borrow_mut().take().unwrap())
    }
}
fn main() {
    let a = std::env::args().collect::<Vec<_>>();
    assert_eq!(
        a.len(),
        4,
        "capture-directory output-directory points-json-or-file"
    );
    let input = std::path::Path::new(&a[1]);
    let output = std::path::Path::new(&a[2]);
    std::fs::create_dir_all(output).unwrap();
    let point_data = std::fs::read_to_string(&a[3]).unwrap_or_else(|_| a[3].clone());
    let points: Vec<[usize; 2]> = serde_json::from_str(&point_data).unwrap();
    assert!(points.iter().all(|p| p[0] < 400 && p[1] < 240));
    trace::configure(points);
    let qualification: serde_json::Value =
        serde_json::from_slice(&std::fs::read(input.join("qualification.json")).unwrap()).unwrap();
    let mut count = 0;
    for (i, submission) in qualification["gpu_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let bytes = std::fs::read(input.join(submission["memory"].as_str().unwrap())).unwrap();
        let mut actual = bytes
            .chunks_exact(2)
            .map(|w| u16::from_le_bytes(w.try_into().unwrap()))
            .collect::<Vec<_>>();
        let mut reference = actual.clone();
        let desc = vliw_command::DeviceSubmission::new(
            submission["base_bytes"].as_u64().unwrap() as u32,
            (submission["count_minus_one"].as_u64().unwrap() as u32 + 1) * 4,
        )
        .unwrap();
        let maximum = submission["maximum_work"].as_u64().unwrap() as usize;
        trace::submission(i);
        functional::Executor::new(maximum)
            .execute(&mut actual, desc)
            .unwrap();
        gpu_v2::system::functional::Executor::new(maximum)
            .execute(&mut reference, desc)
            .unwrap();
        assert_eq!(actual, reference, "observation changed submission {i}");
        let data = actual
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>();
        std::fs::write(output.join(format!("after-{i}.bin")), data).unwrap();
        count += 1;
    }
    eprintln!(
        "{}",
        serde_json::json!({"submissions":count,"all_physical_halfwords_equal_to_original_executor":true,"hardware_timing":false})
    );
}
