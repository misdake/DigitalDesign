#[path = "native_support/traffic.rs"]
mod traffic;
#[test]
fn native_closure_preserves_read_order_masks_and_physical_write_retirement() {
    let run = traffic::run();
    assert!(run.trace.len() > 1200 && run.trace.len() < 3000);
    assert_eq!(run.writes.len(), 25);
    assert_eq!(run.final_words.len(), 1 << 21);
}
