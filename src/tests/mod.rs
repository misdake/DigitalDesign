mod test_utils;

mod adder;
mod binary;
mod external;
mod flatten;
mod reg;

#[cfg(test)]
pub(crate) fn shuffled_list(count: usize, seed: f32) -> Vec<u32> {
    let hash = |v: &u32| -> f32 {
        let v = (*v as f32) * seed;
        v.sin()
    };
    let mut r: Vec<u32> = (0..count).map(|i| i as u32).collect();
    r.sort_by(|a, b| hash(a).total_cmp(&hash(b)));
    // println!("{:?}", r);
    r
}
