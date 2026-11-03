// Observation helpers appended only to a generated diagnostic copy.
pub(super) fn trace_before(
    memory: &super::Memory<'_>,
    xy: [usize; 2],
    fragment: &Fragment,
    state: &DrawState,
    lod: u8,
    mut detail: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let offset = super::column_offset(xy[0], xy[1]);
    let old_color = memory.load(state.target[0] + offset)?;
    let old_depth = memory.load(state.target[1] + offset)?;
    let valid = detail["valid"].as_bool().unwrap();
    let passing = valid
        && match state.render.control & 7 {
            0 => false,
            1 => fragment.depth < old_depth,
            2 => fragment.depth == old_depth,
            3 => fragment.depth <= old_depth,
            4 => fragment.depth > old_depth,
            5 => fragment.depth != old_depth,
            6 => fragment.depth >= old_depth,
            _ => true,
        };
    crate::trace::begin_taps();
    let shaded = if valid {
        Some(shade(
            memory,
            xy,
            fragment,
            state,
            lod,
            &mut Report::default(),
            usize::MAX,
        )?)
    } else {
        None
    };
    let taps = crate::trace::end_taps();
    let alpha = if state.render.control & 16 != 0 {
        ((state.color >> 24) + 2) / 4
    } else {
        64
    };
    let expected_color = if passing {
        let old = expand(old_color);
        let color = shaded.unwrap();
        pack(std::array::from_fn(|c| {
            (((64 - alpha) * u32::from(old[c]) + alpha * u32::from(color[c]) + 32) / 64) as u8
        }))
    } else {
        old_color
    };
    let expected_depth = if passing && state.render.control & 8 != 0 {
        fragment.depth
    } else {
        old_depth
    };
    let context = crate::trace::context();
    for (key,value) in serde_json::json!({
        "context":context, "global_far_q16":state.global.far, "global_depth_shift": 0,"xy":xy,"target_byte":state.target.map(|v|v*2),
        "uv_q16":fragment.uv,"normal":fragment.normal,"incoming_depth":fragment.depth,
        "old_depth":old_depth,"depth_passing":passing,"old_rgb565":old_color,
        "color_shaded":shaded,"expected_rgb565":expected_color,"expected_depth":expected_depth,
        "render":{"texture_byte_base":state.render.texture.base*2,"size_log2":state.render.texture.size_log2,
        "full_mip":state.render.texture.full_mip,"filter":state.render.texture.filter,"fixed_view":state.render.texture.framebuffer,
        "control":state.render.control,"lod_bias":state.render.lod_bias,"lod_q3":lod,
        "uniform_rgba":state.color,"alpha_q6":alpha},"texture_taps":taps
    }).as_object().unwrap() {detail[key]=value.clone();}
    Ok(detail)
}
pub(super) fn trace_after(
    memory: &super::Memory<'_>,
    mut detail: serde_json::Value,
) -> Result<(), String> {
    let xy = detail["xy"].as_array().unwrap();
    let offset = super::column_offset(
        xy[0].as_u64().unwrap() as usize,
        xy[1].as_u64().unwrap() as usize,
    );
    let target = detail["target_byte"].as_array().unwrap();
    let color = memory.load(target[0].as_u64().unwrap() as usize / 2 + offset)?;
    let depth = memory.load(target[1].as_u64().unwrap() as usize / 2 + offset)?;
    assert_eq!(
        u64::from(color),
        detail["expected_rgb565"].as_u64().unwrap()
    );
    assert_eq!(u64::from(depth), detail["expected_depth"].as_u64().unwrap());
    detail["actual_rgb565"] = color.into();
    detail["actual_depth"] = depth.into();
    println!("{}", detail);
    Ok(())
}
