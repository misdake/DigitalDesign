//! Extract whole unmodified modules from the actual portable command factory.
pub fn complete() -> &'static str {
    static ACTUAL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let source = ACTUAL.get_or_init(|| {
        gpu_v2::system::vliw_command::verilog(
            gpu_v2::framebuffer::ports::MaterializedSurface {
                color_base_bytes: 0x100000,
                depth_base_bytes: 0x140000,
                width: 400,
                height: 240,
            },
            gpu_v2::system::pixel::quad_ingress::rtl::Backend::Portable,
        )
        .unwrap()
    });
    source
}
pub fn module(name: &str) -> &'static str {
    let source = complete();
    let marker = format!("module {name}");
    let starts: Vec<_> = source
        .match_indices(&marker)
        .filter_map(|(p, _)| {
            matches!(
                source.as_bytes().get(p + marker.len()),
                Some(b'(' | b' ' | b'\r' | b'\n' | b'#')
            )
            .then_some(p)
        })
        .collect();
    assert_eq!(starts.len(), 1, "{name}");
    let start = starts[0];
    &source[start..start + source[start..].find("endmodule").unwrap() + 9]
}
