#[test]
fn cache_maintenance_hold_is_cpu_local_in_board_wiring() {
    let system = include_str!("../examples/cpu_v3_system/self_test.v");

    assert!(system.contains(".cpu_hold(sysctl_cpu_hold)"));
    assert!(system.contains(".hold(sysctl_cpu_hold || dcache_valid_sweep)"));
    assert_eq!(
        system.matches("sysctl_cpu_hold").count(),
        3,
        "the maintenance hold may only be declared, driven by system control, and consumed by the CPU core"
    );
}
