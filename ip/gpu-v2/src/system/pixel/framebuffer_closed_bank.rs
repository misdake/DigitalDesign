//! Source lowering for the checked cache's aligned, immutable active geometry.
use super::replace_once;
use crate::framebuffer::rtl;
use std::fmt::Write;

fn replace_block(
    source: &mut String,
    start: &str,
    end: &str,
    replacement: &str,
) -> Result<(), String> {
    // Match the complete frozen generic block, not only its section labels.
    let a = rtl::CACHE.find(start).ok_or("Missing bank section start")?;
    let b = rtl::CACHE[a..]
        .find(end)
        .map(|n| a + n)
        .ok_or("Missing bank section end")?;
    replace_once(source, &rtl::CACHE[a..b], replacement)
}

pub(super) fn lower(mut source: String) -> Result<String, String> {
    replace_once(
        &mut source,
        "    reg [1:0]  cbk [0:3];\n    reg [1:0]  dbk [0:3];",
        "    // old/result rows use physical bank order, never lane order.",
    )?;
    replace_once(
        &mut source,
        "wire [15:0] leaf_old_color = oldc[issue];",
        "wire [1:0] leaf_bank = issue ^ {rop_x[1], 1'b0};\n    wire [15:0] leaf_old_color = oldc[leaf_bank];",
    )?;
    replace_once(
        &mut source,
        "wire [1:0]  leaf_out_key;",
        "wire [1:0]  leaf_out_key;\n    wire [1:0] result_bank = leaf_out_key ^ {rop_x[1], 1'b0};",
    )?;
    replace_once(
        &mut source,
        "wire [15:0] leaf_old_depth = oldd[issue];",
        "wire [15:0] leaf_old_depth = oldd[leaf_bank];",
    )?;
    replace_block(
        &mut source,
        "    // ---- Lane geometry for the active quad ----",
        "    // Beat handshakes as combinational views of held old state.",
        "    // A four-pixel maintenance beat has one address in every bank.\n    // Only the half-beat ordering rotates with y[0] = beat[2].\n",
    )?;

    let mut ports = String::from(
        "    // Direct bank geometry. Alignment was checked before publication.\n\
         wire [3:0] maintenance_read_beat = use_nxt ? mt_beat[3:0] + 4'd1 : mt_beat[3:0];\n\
         wire [9:0] maintenance_read_addr = {mt_plane,mt_line,mt_sector,maintenance_read_beat};\n\
         wire [9:0] maintenance_write_addr = {mt_plane,mt_line,mt_sector,mt_beat[3:0]};\n\
         wire rop_read = (rst == RS_RDCOL) || (rst == RS_RDDEP);\n\
         wire rop_write = (rst == RS_WRCOL) || (rst == RS_WRDEP);\n\
         wire rop_depth_read = (rst == RS_RDDEP);\n\
         wire rop_depth_write = (rst == RS_WRDEP);\n",
    );
    for b in 0..4 {
        let h = b / 2;
        let swapped = b ^ 2;
        writeln!(ports, "    wire [15:0] refill_bank{b} = mt_beat[2] ? mem_read_data[{} +: 16] : mem_read_data[{} +: 16];", swapped * 16, b * 16).unwrap();
        writeln!(ports, "    wire [9:0] rop_read_addr{b} = {{rop_depth_read,rop_line,rop_y[3:1],(1'b{h} ^ rop_x[1]),rop_x[3:2]}};").unwrap();
        writeln!(ports, "    wire [9:0] rop_write_addr{b} = {{rop_depth_write,rop_line,rop_y[3:1],(1'b{h} ^ rop_x[1]),rop_x[3:2]}};").unwrap();
        writeln!(ports, "    always @(*) begin\n        rde{b}=1'b0; rda{b}=10'd0; wre{b}=1'b0; wra{b}=10'd0; wrd{b}=16'd0;\n        if(mt_active) begin\n            if(m_rd_issue) begin rde{b}=1'b1; rda{b}=maintenance_read_addr; end\n            if(refill_write) begin wre{b}=1'b1; wra{b}=maintenance_write_addr; wrd{b}=refill_bank{b}; end\n        end else if(rop_active && ce && !fault) begin\n            if(rop_read) begin rde{b}=1'b1; rda{b}=rop_read_addr{b}; end\n            if(rop_write) begin\n                wre{b}=rop_depth_write ? rdw[{b}] : rcw[{b}];\n                wra{b}=rop_write_addr{b};\n                wrd{b}=rop_depth_write ? resd[{b}] : resc[{b}];\n            end\n        end\n    end").unwrap();
    }
    ports.push('\n');
    replace_block(
        &mut source,
        "    // ---- Bank port control (single combinational owner) ----",
        "    // ---- Four explicit synchronous bank read/write processes ----",
        &ports,
    )?;
    replace_once(
        &mut source,
        "    assign mem_write_data  = {rd_data[cur_bank[3]], rd_data[cur_bank[2]],\n                              rd_data[cur_bank[1]], rd_data[cur_bank[0]]};",
        "    assign mem_write_data = mt_beat[2]\n        ? {rd_data[1],rd_data[0],rd_data[3],rd_data[2]}\n        : {rd_data[3],rd_data[2],rd_data[1],rd_data[0]};",
    )?;
    // The previous lane-to-bank registers only hid a constant XOR from
    // synthesis. No new state or read/write phase replaces them.
    let a = rtl::CACHE
        .find("                                cbk[0] <=")
        .unwrap();
    let b = a + rtl::CACHE[a..]
        .find("                                rst <= RS_RDCOL;")
        .unwrap();
    replace_once(&mut source, &rtl::CACHE[a..b], "")?;
    for b in 0..4 {
        replace_once(
            &mut source,
            &format!("oldc[{b}] <= rd_data[cbk[{b}]];"),
            &format!("oldc[{b}] <= rd_data[{b}];"),
        )?;
        replace_once(
            &mut source,
            &format!("oldd[{b}] <= rd_data[dbk[{b}]];"),
            &format!("oldd[{b}] <= rd_data[{b}];"),
        )?;
    }
    for array in ["resc", "resd", "rcw", "rdw"] {
        replace_once(
            &mut source,
            &format!("{array}[leaf_out_key] <="),
            &format!("{array}[result_bank] <="),
        )?;
    }
    Ok(source)
}

pub(super) fn reuse_dead(mut source: String) -> Result<String, String> {
    // RS_CAP captures old bank rows, RS_ISSUE accepts all four inputs, and
    // only RS_WAIT captures terminal returns. The eight-stage leaf cannot
    // return before the last stage-1 input; no old operand remains live.
    replace_once(&mut source, "    reg [15:0] resc [0:3];", "")?;
    replace_once(&mut source, "    reg [15:0] resd [0:3];", "")?;
    replace_once(
        &mut source,
        "resc[result_bank] <= leaf_new_color;",
        "oldc[result_bank] <= leaf_new_color;",
    )?;
    replace_once(
        &mut source,
        "resd[result_bank] <= rowbuf[{leaf_out_key, 1'b1}][15:0];",
        "oldd[result_bank] <= rowbuf[{leaf_out_key, 1'b1}][15:0];",
    )?;
    for b in 0..4 {
        replace_once(
            &mut source,
            &format!("wrd{b}=rop_depth_write ? resd[{b}] : resc[{b}];"),
            &format!("wrd{b}=rop_depth_write ? oldd[{b}] : oldc[{b}];"),
        )?;
    }
    Ok(source)
}

pub(super) fn loan_color(mut source: String) -> Result<String, String> {
    // oc1 is still required by the real blend expand. A true terminal write
    // always uses quantized RGB, while a false write flag suppresses the cache
    // bank write completely; only the fallback payload becomes unspecified.
    for stage in 2..=7 {
        replace_once(&mut source, &format!("reg [15:0] oc{stage};"), "")?;
        replace_once(
            &mut source,
            &format!("oc{stage} <= oc{};", stage - 1),
            "// Old color remains in cache; a disabled write needs no fallback.",
        )?;
    }
    replace_once(
        &mut source,
        "new_color <= gate7 ? {qr7, qg7, qb7} : oc7;",
        "new_color <= {qr7, qg7, qb7}; // Cache consumes only when color_written.",
    )?;
    Ok(source)
}

pub(super) fn stream(mut source: String) -> Result<String, String> {
    replace_once(
        &mut source,
        "// `{head,rowc[2:0]}`; there is no parallel eight-row snapshot. ROP progression",
        "// `{head,lane,phase}`; pairs are borrowed from the live head. ROP progression",
    )?;
    replace_once(&mut source, "    reg [3:0]  rowc;", "")?;
    replace_once(
        &mut source,
        "    reg [31:0] rowbuf [0:7];",
        "    reg [31:0] source_rgba; // Sole captured half of the paid row pair.",
    )?;
    replace_once(
        &mut source,
        "    wire [6:0]  leaf_row_sel = {issue, 1'b0};",
        "",
    )?;
    for (channel, slice) in [("r", "7:0"), ("g", "15:8"), ("b", "23:16"), ("a", "31:24")] {
        replace_once(
            &mut source,
            &format!("leaf_src_{channel} = rowbuf[leaf_row_sel][{slice}];"),
            &format!("leaf_src_{channel} = source_rgba[{slice}];"),
        )?;
    }
    replace_once(
        &mut source,
        "leaf_src_depth = rowbuf[leaf_row_sel + 7'd1][15:0];",
        "leaf_src_depth = ordout[15:0];",
    )?;
    replace_block(
        &mut source,
        "    // ---- Row payload registered read, combinational row address ----",
        "    // ---- Memory request / write data are combinational views of held state ----",
        "    // Immutable head remains owned until RS_DONE. CAP reads lane0 RGBA;\n\
         // RS_ROWS captures RGBA and reads depth; RS_ISSUE accepts the pair and\n\
         // reads the next RGBA. One held DO pays each synchronous read on CE0.\n\
         wire [1:0] source_color_lane = (rst == RS_ISSUE) ? issue + 2'd1 : 2'd0;\n\
         wire [3:0] row_raddr = (rst == RS_ROWS)\n\
             ? {head,issue,1'b1} : {head,source_color_lane,1'b0};\n\
         wire row_ren = ce && !fault && ((rst == RS_CAP) || (rst == RS_ROWS)\n\
             || ((rst == RS_ISSUE) && issue != 2'd3 && leaf_in_ready));\n\
         always @(posedge clk) begin\n\
             if(reset) ordout <= 32'd0;\n\
             else if(row_ren) ordout <= oram[row_raddr];\n\
         end\n\n",
    )?;
    replace_once(
        &mut source,
        "                            rst <= RS_ROWS;",
        "                            rst <= RS_RDCOL;",
    )?;
    replace_once(&mut source, "                            rowc <= 4'd0;", "")?;
    // This block has already lost its cbk/dbk assignments in the bank lowering.
    let start = "                        RS_ROWS: begin";
    let end = "                        RS_RDCOL: rst <= RS_RDDEP;";
    let a = source.find(start).ok_or("Missing snapshot state")?;
    let b = a + source[a..].find(end).ok_or("Missing snapshot state end")?;
    let anchor = source[a..b].to_owned();
    replace_once(
        &mut source,
        &anchor,
        "                        RS_ROWS: begin\n\
                                    source_rgba <= ordout;\n\
                                    rst <= RS_ISSUE;\n\
                                end\n",
    )?;
    replace_once(
        &mut source,
        "                            rst <= RS_ISSUE;",
        "                            rst <= RS_ROWS;",
    )?;
    replace_once(
        &mut source,
        "                                else issue <= issue + 2'd1;",
        "                                else begin issue <= issue + 2'd1; rst <= RS_ROWS; end",
    )?;
    replace_once(
        &mut source,
        "resd[result_bank] <= rowbuf[{leaf_out_key, 1'b1}][15:0];",
        "resd[result_bank] <= leaf_new_depth;",
    )?;
    // Source depth is no longer borrowed at return: six CE-held transports
    // retain it until the keyed terminal result. Old depth stays early-only.
    for stage in 2..=7 {
        replace_once(
            &mut source,
            &format!("reg        v{stage};"),
            &format!("reg [15:0] sd{stage};\n    reg        v{stage};"),
        )?;
        replace_once(
            &mut source,
            &format!("k{stage} <= k{};", stage - 1),
            &format!(
                "k{stage} <= k{};\n            sd{stage} <= sd{};",
                stage - 1,
                stage - 1
            ),
        )?;
    }
    replace_once(
        &mut source,
        "new_depth <= 16'd0; // Closed cache borrows depth; this port is unused.",
        "new_depth <= sd7; // Cache consumes source depth only for a true DW flag.",
    )?;
    if source.contains("rowbuf") || source.contains("rowc") {
        return Err("Whole-quad snapshot reference remains".into());
    }
    Ok(source)
}
