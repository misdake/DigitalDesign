//! Closed Public16 weight leases; generic packet/cache modules stay intact.
fn body(source: &str, name: &str) -> Result<String, String> {
    let begin = source
        .find(&format!("module {name} "))
        .or_else(|| source.find(&format!("module {name}(")))
        .ok_or_else(|| format!("missing {name}"))?;
    let end = begin + source[begin..].find("endmodule").ok_or("module end")? + 9;
    Ok(source[begin..end].to_owned())
}
fn one(value: &mut String, old: &str, new: &str) -> Result<(), String> {
    if value.matches(old).count() != 1 {
        return Err(format!("unique weights anchor: {old}"));
    }
    *value = value.replace(old, new);
    Ok(())
}
pub fn lower(mut source: String) -> Result<String, String> {
    source = source.replace("\r\n", "\n");
    let mut backend = body(&source, "gpu_v2_texture_runtime_backend")?;
    backend = backend
        .replace(
            "gpu_v2_texture_runtime_backend(",
            "gpu_v2_texture_runtime_backend_unique(",
        )
        .replace(
            "gpu_v2_runtime_packet_backend_shared packets(",
            "gpu_v2_runtime_packet_backend_unique packets(",
        );
    one(&mut backend,"output wire out_valid,output wire[71:0] out_payload,","output wire out_valid,output wire[39:0] out_payload,\n output wire weight_write,output wire[6:0]weight_address,output wire[35:0]weight_value,")?;
    one(&mut backend," reg stopped,coarse_next;"," assign weight_write=advance&&member_return;\n assign weight_address={member_payload[71:68],member_payload[73:72],member_payload[64]};\n assign weight_value=member_payload[35:0];\n reg stopped,coarse_next;")?;
    let mut packets = body(&source, "gpu_v2_runtime_packet_backend_shared")?;
    packets = packets
        .replace(
            "gpu_v2_runtime_packet_backend_shared(",
            "gpu_v2_runtime_packet_backend_unique(",
        )
        .replace("[71:0]", "[39:0]")
        .replace("gpu_v2_texture_work #", "gpu_v2_texture_work_unique #")
        .replace("packet_leaf #", "packet_leaf_unique #")
        .replace(
            "gpu_v2_packet_store_ordered u_pool(",
            "gpu_v2_packet_store_unique u_pool(",
        );
    let mut work = body(&source, "gpu_v2_texture_work")?;
    work = work.replace("gpu_v2_texture_work #", "gpu_v2_texture_work_unique #");
    one(
        &mut work,
        "reg [71:0] payload_rows",
        "reg [35:0] payload_rows",
    )?;
    one(
        &mut work,
        "wire [71:0] payload_data",
        "wire [35:0] payload_data",
    )?;
    one(
        &mut work,
        "{payload_data[71:36], (consume_head ? head1 : head0), payload_data[35:0]}",
        "{payload_data, (consume_head ? head1 : head0), 36'd0}",
    )?;
    one(
        &mut work,
        "{in_member[75:40], in_member[35:0]}",
        "in_member[75:40]",
    )?;
    let mut pool = body(&source, "gpu_v2_packet_store_ordered")?;
    pool = pool
        .replace(
            "gpu_v2_packet_store_ordered #",
            "gpu_v2_packet_store_unique #",
        )
        .replace("[71:0]", "[39:0]");
    let start = pool
        .find(" generate if(NATIVE)begin:payload_hardware")
        .ok_or("pool hardware")?;
    pool.replace_range(
        start..pool.len() - 9,
        include_str!("postquad_unique_weights_pool.v"),
    );
    let mut cache = body(&source, "gpu_v2_texture_runtime_cache")?;
    cache = cache.replace(
        "gpu_v2_texture_runtime_cache #",
        "gpu_v2_texture_runtime_cache_unique #",
    );
    one(&mut cache,"input wire in_valid,input wire[71:0]in_payload,","input wire in_valid,input wire[39:0]in_payload,\n input wire weight_write,input wire[6:0]weight_address,input wire[35:0]weight_value,")?;
    one(
        &mut cache,
        "reg[71:0]res_payload,paid_payload;",
        "reg[39:0]res_payload,paid_payload;reg[35:0]res_weights;",
    )?;
    one(
        &mut cache,
        "wire[71:0]head_payload=in_payload;",
        "wire[39:0]head_payload=in_payload;",
    )?;
    let begin = cache
        .find(" // Inverse mapping on36 coefficient bits")
        .ok_or("cache inverse")?;
    let end = cache[begin..]
        .find(" assign mem_req_valid")
        .ok_or("cache output end")?
        + begin;
    cache.replace_range(begin..end, include_str!("postquad_unique_weights_cache.v"));
    one(
        &mut cache,
        "res_payload<=paid_payload;res_valid<=1;rd_valid<=0;",
        "res_payload<=paid_payload;res_weights<=weight_do;res_valid<=1;rd_valid<=0;",
    )?;
    let mut front = body(&source, "gpu_v2_pixel_sampling_ingress")?;
    front = front
        .replace(
            "gpu_v2_texture_runtime_backend backend(",
            "gpu_v2_texture_runtime_backend_unique backend(",
        )
        .replace(
            "gpu_v2_texture_runtime_cache demand(",
            "gpu_v2_texture_runtime_cache_unique demand(",
        );
    one(
        &mut front,
        "output wire[71:0]packet_out_payload,",
        "output wire[39:0]packet_out_payload,",
    )?;
    one(&mut front," gpu_v2_texture_runtime_backend_unique backend("," wire weight_write;wire[6:0]weight_address;wire[35:0]weight_value;\n gpu_v2_texture_runtime_backend_unique backend(")?;
    one(&mut front,".coefficient_weights(),.coefficient_meta(),.member_payload(),.packet_capture(),.work_ack());",".coefficient_weights(),.coefficient_meta(),.member_payload(),.packet_capture(),.work_ack(),\n .weight_write(weight_write),.weight_address(weight_address),.weight_value(weight_value));")?;
    one(&mut front,".in_valid(packet_out_valid),.in_payload(packet_out_payload),",".weight_write(weight_write),.weight_address(weight_address),.weight_value(weight_value),\n .in_valid(packet_out_valid),.in_payload(packet_out_payload),")?;
    let old = body(&source, "gpu_v2_pixel_sampling_ingress")?;
    one(&mut source, &old, &front)?;
    source.push_str(&format!(
        "\n{backend}\n{packets}\n{work}\n{pool}\n{cache}\n{}",
        include_str!("postquad_unique_weights_packet.v")
    ));
    Ok(source)
}
