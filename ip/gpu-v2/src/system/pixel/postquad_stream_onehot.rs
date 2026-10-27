//! Closed reset-led tag lookup; generic cache keeps its priority semantics.
pub fn lower(source: String) -> Result<String, String> {
    let old = r#"    reg        hit;
    reg [2:0]  hit_line;
    reg        free_found;
    reg [2:0]  free_line;
    always @(*) begin
        hit = 1'b0;
        hit_line = 3'd0;
        free_found = 1'b0;
        free_line = 3'd0;
        for (k = 0; k < 8; k = k + 1) begin
            if (tvalid[k] && tag[k] == head_tile) begin
                hit = 1'b1;
                hit_line = k[2:0];
            end
            if (!tvalid[k] && !free_found) begin
                free_found = 1'b1;
                free_line = k[2:0];
            end
        end
    end"#;
    let new = r#"    // Single maintenance owner publishes absent targets, so valid tags
    // are unique in this explicit reset-led profile. Free choices are not.
    wire [7:0] resident_hits;
    genvar resident_index;
    generate for(resident_index=0;resident_index<8;resident_index=resident_index+1)begin:resident_compare
        assign resident_hits[resident_index]=tvalid[resident_index]&&tag[resident_index]==head_tile;
    end endgenerate
    wire hit=|resident_hits;
    wire [2:0] hit_line={|resident_hits[7:4],
        resident_hits[2]|resident_hits[3]|resident_hits[6]|resident_hits[7],
        resident_hits[1]|resident_hits[3]|resident_hits[5]|resident_hits[7]};
    reg        free_found;
    reg [2:0]  free_line;
    always @(*) begin
        free_found = 1'b0;
        free_line = 3'd0;
        for (k = 0; k < 8; k = k + 1) begin
            if (!tvalid[k] && !free_found) begin
                free_found = 1'b1;
                free_line = k[2:0];
            end
        end
    end"#;
    if source.matches(old).count() != 1 {
        return Err("Closed onehot tag lookup anchor".into());
    }
    Ok(source.replacen(old, new, 1))
}
