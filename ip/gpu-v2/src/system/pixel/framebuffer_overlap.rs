//! Ordered overlap of two existing immutable Output2 slots.
use super::replace_once;

pub(super) fn lower(mut source: String) -> Result<String, String> {
    replace_once(
        &mut source,
        "    reg        rop_blend;",
        r#"    reg        rop_blend;
    // No second result owner: all prior returns and writes precede handover.
    localparam A_IDLE=3'd0,A_RDCOL=3'd1,A_RDDEP=3'd2,A_CAP=3'd3,
               A_ROWS=3'd4,A_ISSUE=3'd5,A_DONE=3'd6;
    reg [2:0] ahead;
    wire next_slot = ~head;
    wire ahead_active = (ahead != A_IDLE);
    wire ahead_eligible = slot_ready[next_slot]
        && s_hx[next_slot][8:4] == rop_x[8:4]
        && s_hy[next_slot][7:4] == rop_y[7:4]
        && {s_hx[next_slot][3:0],s_hy[next_slot][3:0]}
            != {rop_x[3:0],rop_y[3:0]};
    wire [8:0] operand_x = ahead_active ? s_hx[next_slot] : rop_x;
    wire [3:0] operand_mask = ahead_active ? s_hm[next_slot] : rop_mask;
    wire [2:0] operand_func = ahead_active ? s_func[next_slot] : rop_func;
    wire operand_dw = ahead_active ? s_dw[next_slot] : rop_dw;
    wire operand_blend = ahead_active ? s_blend[next_slot] : rop_blend;"#,
    )?;
    for (a, b) in [
        (
            "wire        leaf_in_valid = (rst == RS_ISSUE);",
            "wire leaf_in_valid = (rst == RS_ISSUE) || (ahead == A_ISSUE);",
        ),
        (
            "wire [1:0] leaf_bank = issue ^ {rop_x[1], 1'b0};",
            "wire [1:0] leaf_bank = issue ^ {operand_x[1], 1'b0};",
        ),
        (
            "wire        leaf_covered = rop_mask[issue];",
            "wire leaf_covered = operand_mask[issue];",
        ),
        (".blend(rop_blend)", ".blend(operand_blend)"),
        (".depth_func(rop_func)", ".depth_func(operand_func)"),
        (".depth_write(rop_dw)", ".depth_write(operand_dw)"),
    ] {
        replace_once(&mut source, a, b)?;
    }
    for bank in 0..4 {
        let half = bank / 2;
        replace_once(&mut source,
            &format!("wire [9:0] rop_read_addr{bank} = {{rop_depth_read,rop_line,rop_y[3:1],(1'b{half} ^ rop_x[1]),rop_x[3:2]}};"),
            &format!("wire [9:0] rop_read_addr{bank} = {{rop_depth_read,rop_line,rop_y[3:1],(1'b{half} ^ rop_x[1]),rop_x[3:2]}};\n    wire [9:0] ahead_read_addr{bank} = {{(ahead == A_RDDEP),rop_line,s_hy[next_slot][3:1],(1'b{half} ^ s_hx[next_slot][1]),s_hx[next_slot][3:2]}};"))?;
        replace_once(&mut source,
            &format!("if(rop_read) begin rde{bank}=1'b1; rda{bank}=rop_read_addr{bank}; end"),
            &format!("if(rop_read) begin rde{bank}=1'b1; rda{bank}=rop_read_addr{bank}; end\n            if(ahead == A_RDCOL || ahead == A_RDDEP) begin rde{bank}=1'b1; rda{bank}=ahead_read_addr{bank}; end"))?;
    }
    let a = source
        .find("    // Immutable head remains owned until RS_DONE.")
        .ok_or("Missing stream read")?;
    let b = a + source[a..]
        .find("    // ---- Memory request / write data")
        .ok_or("Missing stream read end")?;
    let anchor = source[a..b].to_owned();
    replace_once(
        &mut source,
        &anchor,
        r#"    // Current and next Output2 slots remain immutable until actual commit.
    // A paid synchronous DO is held on CE0; no second row snapshot is added.
    wire source_rows = ahead_active ? (ahead == A_ROWS) : (rst == RS_ROWS);
    wire source_issue = ahead_active ? (ahead == A_ISSUE) : (rst == RS_ISSUE);
    wire source_cap = ahead_active ? (ahead == A_CAP) : (rst == RS_CAP);
    wire [1:0] source_color_lane = source_issue ? issue + 2'd1 : 2'd0;
    wire source_slot = ahead_active ? next_slot : head;
    wire [3:0] row_raddr = source_rows
        ? {source_slot,issue,1'b1} : {source_slot,source_color_lane,1'b0};
    wire row_ren = ce && !fault && (source_cap || source_rows
        || (source_issue && issue != 2'd3 && leaf_in_ready));
    always @(posedge clk) begin
        if(reset) ordout <= 32'd0;
        else if(row_ren) ordout <= oram[row_raddr];
    end

"#,
    )?;
    replace_once(
        &mut source,
        "\n            rst <= RS_IDLE;",
        "\n            rst <= RS_IDLE;\n            ahead <= A_IDLE;",
    )?;
    replace_once(
        &mut source,
        "\n                rst <= RS_IDLE;",
        "\n                rst <= RS_IDLE;\n                ahead <= A_IDLE;",
    )?;
    replace_once(
        &mut source,
        "                // ---- ROP sequencer ----",
        r#"                // Dead old operands can prepare the immutable second slot.
                // Only distinct aligned quads in the pinned current tile qualify.
                // No line maintenance starts while either quad is in flight.
                if (rop_active && rst == RS_WAIT && ahead == A_IDLE && ahead_eligible) begin
                    ahead <= A_RDCOL;
                    issue <= 2'd0;
                end
                case (ahead)
                    A_RDCOL: ahead <= A_RDDEP;
                    A_RDDEP: begin
                        oldc[0]<=rd_data[0]; oldc[1]<=rd_data[1];
                        oldc[2]<=rd_data[2]; oldc[3]<=rd_data[3];
                        ahead <= A_CAP;
                    end
                    A_CAP: begin
                        oldd[0]<=rd_data[0]; oldd[1]<=rd_data[1];
                        oldd[2]<=rd_data[2]; oldd[3]<=rd_data[3];
                        ahead <= A_ROWS;
                    end
                    A_ROWS: begin source_rgba<=ordout; ahead<=A_ISSUE; end
                    A_ISSUE: if(leaf_in_ready) begin
                        if(issue==2'd3) ahead<=A_DONE;
                        else begin issue<=issue+2'd1; ahead<=A_ROWS; end
                    end
                    default: begin end
                endcase

                // ---- ROP sequencer ----"#,
    )?;
    replace_once(&mut source,"                            rop_active <= 1'b0;\n                            rst <= RS_IDLE;",r#"                            if(ahead_active) begin
                                // Retire only the old owner, then promote the
                                // next slot's exact paid-read phase. The next
                                // terminal return is at least one edge later.
                                rop_x<=s_hx[next_slot]; rop_y<=s_hy[next_slot];
                                rop_mask<=s_hm[next_slot]; rop_func<=s_func[next_slot];
                                rop_dw<=s_dw[next_slot]; rop_blend<=s_blend[next_slot];
                                got<=4'd0; ahead<=A_IDLE;
                                case(ahead)
                                    A_RDCOL: rst<=RS_RDDEP;
                                    A_RDDEP: rst<=RS_CAP;
                                    A_CAP: rst<=RS_ROWS;
                                    A_ROWS: rst<=RS_ISSUE;
                                    A_ISSUE: if(issue==2'd3) rst<=RS_WAIT; else rst<=RS_ROWS;
                                    A_DONE: rst<=RS_WAIT;
                                    default: rst<=RS_IDLE;
                                endcase
                            end else begin
                                rop_active <= 1'b0;
                                rst <= RS_IDLE;
                            end"#)?;
    Ok(source)
}
