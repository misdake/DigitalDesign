//! Closed resident prefix flow: two descriptor owners, no result snapshot.
use super::replace_once;
use std::fmt::Write;

fn section(source: &mut String, start: &str, end: &str, replacement: &str) -> Result<(), String> {
    let a = source
        .find(start)
        .ok_or_else(|| format!("Missing prefix section {start}"))?;
    let b = a + source[a..]
        .find(end)
        .ok_or_else(|| format!("Missing prefix end {end}"))?;
    let anchor = source[a..b].to_owned();
    replace_once(source, &anchor, replacement)
}

pub(super) fn lower(mut source: String) -> Result<String, String> {
    // Preserve the wall-clock maintenance body verbatim, changing only its
    // compute admission: an offered miss may start after all owners retire.
    let start = source
        .find("                // ---- Start a quad or a demand refill ----")
        .ok_or("Missing start")?;
    let end = start
        + source[start..]
            .find("                // ---- ROP sequencer ----")
            .ok_or("Missing sequencer")?;
    let block = source[start..end].to_owned();
    let miss = block
        .find("                            mt_active <= 1'b1;")
        .ok_or("Missing miss")?;
    let flush = block
        .find("                    end else if (flushing) begin")
        .ok_or("Missing flush")?;
    let miss_end = block[..flush]
        .rfind("                        end\n")
        .ok_or("Missing miss end")?;
    let replacement=format!("                // Refill/evict/flush never overlap a reserved prefix or return owner.\n                if (!mt_active && !rop_active) begin\n                    if(row_valid && fill_row==0 && !flushing && !hit && !ingress_error) begin\n{}{}", &block[miss..miss_end], &block[flush..]);
    replace_once(&mut source, &block, &replacement)?;
    section(
        &mut source,
        "                // ---- ROP sequencer ----",
        "\n            end\n        end\n    end\nendmodule",
        include_str!("framebuffer_prefix_control.v"),
    )?;
    // Source payload is borrowed from the offered row pair, not copied into RAM.
    replace_once(&mut source,"                        oram[{tail, row_index}] <= row_data;",
        "                        if(!row_index[0]) source_rgba<=row_data;\n                        if(row_index==0) begin prep<=P_COLOR; rop_line<=hit_line; end\n                        if(row_index==1) prep<=P_DEPTH;")?;
    section(
        &mut source,
        "    // ---- ROP sequencer state ----",
        "    // ---- Maintenance sequencer state ----",
        r#"    // Two immutable headers are the only outstanding quad descriptors.
    // The current tail is a private prefix; the head owns ordered returns.
    wire rop_active = slot_ready[0] || slot_ready[1] || (fill_row!=0);
    reg [2:0] rop_line;
    reg [31:0] source_rgba;
    reg [15:0] oldc[0:3],oldd[0:3];
    localparam P_IDLE=2'd0,P_COLOR=2'd1,P_DEPTH=2'd2;
    reg [1:0] prep;
    reg depth_pending;
    reg [15:0] depth_data;
    reg [1:0] depth_key,return_key;
    reg depth_flag,quad_color_dirty,quad_depth_dirty;
    wire ingress_error,extra_read;
    wire source_ready,source_bad,leaf_ce,return_bad;
    wire raw_retire = ce && !fault && depth_pending && depth_key==2'd3;
    wire row_credit = !slot_ready[tail] || (raw_retire && tail==head);
    wire row0_fire = row_accept && row_index==0 && !source_bad;
    wire depth0_fire = row_accept && row_index==1 && !source_bad;

"#,
    )?;
    // Removed scalar sequencer state has no diagnostic-only retained replica.
    for old in [
        "\n            rop_active <= 1'b0;\n",
        "\n                rop_active <= 1'b0;\n",
        "\n            rst <= RS_IDLE;\n",
        "\n                rst <= RS_IDLE;\n",
        "\n            issue <= 2'd0;\n",
        "\n            got <= 4'd0;\n",
    ] {
        replace_once(&mut source, old, "\n")?;
    }
    replace_once(
        &mut source,
        "    reg [31:0] oram [0:15];\n    reg [31:0] ordout;",
        "    // No Output2 payload RAM: arithmetic consumes accepted row pairs.",
    )?;
    section(
        &mut source,
        "    // Immutable head remains owned until RS_DONE.",
        "    // ---- Memory request / write data",
        "",
    )?;
    replace_once(&mut source,"wire [8:0] head_tile = ({1'b0, s_hy[head]} >> 4) * (WIDTH >> 4)\n        + ({5'd0, s_hx[head]} >> 4);",
        "wire [8:0] head_tile = ({1'b0,row_y} >> 4) * (WIDTH >> 4) + ({5'd0,row_x} >> 4);")?;
    replace_once(
        &mut source,
        "wire ingress_error = ce && row_valid && !flushing && !full && (",
        "assign ingress_error = ce && row_valid && !flushing && row_credit && (",
    )?;
    replace_once(
        &mut source,
        "wire extra_read = mt_active && mem_read_valid",
        "assign extra_read = mt_active && mem_read_valid",
    )?;
    replace_once(
        &mut source,
        "assign row_accept = ce && !flushing && !fault && !full && row_valid;",
        "assign row_accept = ce && !flushing && !fault && row_credit && source_ready && row_valid && !source_bad;",
    )?;
    replace_once(
        &mut source,
        "if (row_valid && !flushing && !full) begin",
        "if (row_valid && !flushing && row_credit && source_ready) begin",
    )?;
    replace_once(
        &mut source,
        "if (ce && !fault) begin",
        "if (ce && !fault && !source_bad) begin",
    )?;
    replace_once(
        &mut source,
        "            // ---- Compute boundary:",
        r#"            // Both native bank DO groups are paid wall-clock returns. Prefix
            // stalls or CE0 retain the group; no second read is fabricated.
            if(prep==P_COLOR) begin
                oldc[0]<=rd_data[0];oldc[1]<=rd_data[1];
                oldc[2]<=rd_data[2];oldc[3]<=rd_data[3];
            end
            if(prep==P_DEPTH) begin
                oldd[0]<=rd_data[0];oldd[1]<=rd_data[1];
                oldd[2]<=rd_data[2];oldd[3]<=rd_data[3];prep<=P_IDLE;
            end
            if(return_bad||ingress_error)fault_c<=1'b1;
            // ---- Compute boundary:"#,
    )?;
    replace_once(&mut source,"            victim <= 3'd0;", "            victim <= 3'd0;\n            prep<=P_IDLE;depth_pending<=0;return_key<=0;\n            quad_color_dirty<=0;quad_depth_dirty<=0;")?;
    replace_once(&mut source,"            if (fault) begin", "            if (fault) begin\n                prep<=P_IDLE;depth_pending<=0;return_key<=0;\n                quad_color_dirty<=0;quad_depth_dirty<=0;")?;
    // First C pays stage 1; first D pays compare stage 2. Later lanes read the
    // captured operands, whose previous quad lifetime ended at its last input.
    for (old,new) in [
        ("wire        leaf_in_valid = (rst == RS_ISSUE);", "wire leaf_in_valid = row_accept && row_index[0] && !source_bad;"),
        ("wire [1:0]  leaf_in_key = issue;", "wire [1:0] leaf_in_key = row_index[2:1];"),
        ("wire [1:0] leaf_bank = issue ^ {rop_x[1], 1'b0};", "wire [1:0] leaf_bank = row_index[2:1] ^ {s_hx[tail][1],1'b0};"),
        ("wire [15:0] leaf_old_color = oldc[leaf_bank];", "wire [15:0] leaf_old_color = (row_index[2:1]==0) ? rd_data[leaf_bank] : oldc[leaf_bank];"),
        ("wire [1:0] result_bank = leaf_out_key ^ {rop_x[1], 1'b0};", "wire [1:0] result_bank = leaf_out_key ^ {s_hx[head][1],1'b0};"),
        ("leaf_src_depth = ordout[15:0];", "leaf_src_depth = row_data[15:0];"),
        ("wire        leaf_covered = rop_mask[issue];", "wire leaf_covered = s_hm[tail][row_index[2:1]];"),
        (".blend(rop_blend)", ".blend(s_blend[tail])"),
        (".depth_func(rop_func)", ".depth_func(s_func[tail])"),
        (".depth_write(rop_dw)", ".depth_write(s_dw[tail])"),
        (".ce(ce && !fault)", ".ce(leaf_ce)"),
        (".old_depth(leaf_old_depth),", ".old_depth(leaf_old_depth),\n        .first_old_depth(rd_data[{s_hx[tail][1],1'b0}]),"),
        ("    input      [15:0] old_depth,", "    input      [15:0] old_depth,\n    input      [15:0] first_old_depth,"),
        ("gate2 <= covered1 && depth_pass(func1, sd1, od1);", "gate2 <= covered1 && depth_pass(func1, sd1, compared_old_depth);"),
        ("    // ---- Stage 2: compare/expand ----", "    wire [15:0] compared_old_depth = (k1==0) ? first_old_depth : od1;\n    // ---- Stage 2: compare/expand ----"),
    ] {replace_once(&mut source,old,new)?;}
    let mut ports = String::from(
        r#"    // Incoming old operands are read only for an accepted first row/pair.
    wire [8:0] active_tile = ({1'b0,s_hy[head]} >> 4)*(WIDTH>>4) + ({5'd0,s_hx[head]}>>4);
    wire owner0_overlap = slot_ready[0] && !(raw_retire && head==0)
        && {row_x[3:0],row_y[3:0]}=={s_hx[0][3:0],s_hy[0][3:0]};
    wire owner1_overlap = slot_ready[1] && !(raw_retire && head==1)
        && {row_x[3:0],row_y[3:0]}=={s_hx[1][3:0],s_hy[1][3:0]};
    assign source_ready = (fill_row!=0) || (!mt_active && hit
        && (!rop_active || (head_tile==active_tile && !owner0_overlap && !owner1_overlap)));
    // Raw proposal is independent from its validation, so no CE feedback loop.
    wire leaf_proposal = ce && !fault && ((fill_row==0)
        || (row_valid && row_credit && source_ready));
    wire return_proposed = leaf_proposal && leaf_out_valid;
    assign return_bad = return_proposed && (depth_pending || !slot_ready[head]
        || leaf_out_key!=return_key);
    assign source_bad = ingress_error || extra_read || return_bad;
    assign leaf_ce = leaf_proposal && !source_bad;
    wire return_fire = leaf_ce && leaf_out_valid;
    wire depth_fire = ce && !fault && !source_bad && depth_pending;
    wire [1:0] depth_bank = depth_key ^ {s_hx[head][1],1'b0};
    wire [3:0] maintenance_read_beat = use_nxt ? mt_beat[3:0]+4'd1 : mt_beat[3:0];
    wire [9:0] maintenance_read_addr = {mt_plane,mt_line,mt_sector,maintenance_read_beat};
    wire [9:0] maintenance_write_addr = {mt_plane,mt_line,mt_sector,mt_beat[3:0]};
"#,
    );
    for b in 0..4 {
        let h = b / 2;
        let swap = b ^ 2;
        writeln!(ports,"    wire [15:0] refill_bank{b} = mt_beat[2] ? mem_read_data[{} +: 16] : mem_read_data[{} +: 16];",swap*16,b*16).unwrap();
        writeln!(ports,"    wire [9:0] source_addr{b} = {{depth0_fire,(row0_fire ? hit_line : rop_line),row_y[3:1],(1'b{h} ^ row_x[1]),row_x[3:2]}};").unwrap();
        writeln!(ports,"    wire [9:0] result_addr{b} = {{depth_fire,rop_line,s_hy[head][3:1],(1'b{h} ^ s_hx[head][1]),s_hx[head][3:2]}};").unwrap();
        writeln!(ports,"    always @(*) begin\n        rde{b}=0;rda{b}=0;wre{b}=0;wra{b}=0;wrd{b}=0;\n        if(mt_active) begin\n            if(m_rd_issue)begin rde{b}=1;rda{b}=maintenance_read_addr;end\n            if(refill_write)begin wre{b}=1;wra{b}=maintenance_write_addr;wrd{b}=refill_bank{b};end\n        end else begin\n            if(row0_fire||depth0_fire)begin rde{b}=1;rda{b}=source_addr{b};end\n            if(return_fire&&leaf_cw&&result_bank==2'd{b})begin wre{b}=1;wra{b}=result_addr{b};wrd{b}=leaf_new_color;end\n            if(depth_fire&&depth_flag&&depth_bank==2'd{b})begin wre{b}=1;wra{b}=result_addr{b};wrd{b}=depth_data;end\n        end\n    end").unwrap();
    }
    section(
        &mut source,
        "    // Direct bank geometry.",
        "    // ---- Four explicit synchronous bank read/write processes ----",
        &ports,
    )?;
    Ok(source)
}
