//! Experimental command-profile texture layout. Generic samplers stay unchanged.
//! External16x16 RGB565 tiles, resident16x8 lines, 8sets x4ways, 8KiB data.

fn once(text: &mut String, old: &str, new: &str) -> Result<(), String> {
    if text.matches(old).count() != 1 {
        return Err(format!("Texture16x8 anchor: {old}"));
    }
    *text = text.replacen(old, new, 1);
    Ok(())
}

fn edit(
    source: &mut String,
    name: &str,
    f: impl FnOnce(&mut String) -> Result<(), String>,
) -> Result<(), String> {
    let start = source
        .find(&format!("module {name}("))
        .or_else(|| source.find(&format!("module {name} #")))
        .ok_or_else(|| format!("Texture16x8 module {name}"))?;
    let end = start
        + source[start..]
            .find("endmodule")
            .ok_or("Texture16x8 module end")?
        + 9;
    let mut body = source[start..end].to_owned();
    f(&mut body)?;
    source.replace_range(start..end, &body);
    Ok(())
}

pub fn lower(mut source: String) -> Result<String, String> {
    edit(&mut source, "membership_leaf", |s| {
        once(
            s,
            "wire sx=(common0[63:60]<=3)||(common0[52:50]!=7);",
            "wire sx=(common0[63:60]<=4)||(common0[52:49]!=15);",
        )?;
        once(
            s,
            "in_c2[2:0],in_c0[2:0],in_c2[9:3],in_c0[9:3]",
            "in_c2[2:0],in_c0[3:0],in_c2[9:3],in_c0[9:4]",
        )
    })?;
    edit(&mut source, "packet_leaf_unique", |s| {
        *s = include_str!("texture_packet16x8.v").to_owned();
        Ok(())
    })?;
    // H is the canonical256B line address. Its15 bits plus7 localXY bits
    // retain Group35 and the qualified native FIFO/lookahead calendar.
    for name in [
        "gpu_v2_packet_store_unique",
        "gpu_v2_runtime_packet_backend_unique",
        "gpu_v2_texture_runtime_backend_unique",
        "gpu_v2_vliw_sampling",
    ] {
        edit(&mut source, name, |s| {
            *s = s
                .replace("[15:0]next_key", "[14:0]next_key")
                .replace("[15:0] next_key", "[14:0] next_key")
                .replace("[15:0]packet_next_key", "[14:0]packet_next_key");
            if name == "gpu_v2_packet_store_unique" {
                once(
                    s,
                    "assign next_key=data_do[15:0]",
                    "assign next_key=data_do[14:0]",
                )?;
            }
            Ok(())
        })?;
    }
    edit(&mut source, "gpu_v2_texture_runtime_cache_unique", |s| {
        for (old,new) in [
            ("[15:0]next_key","[14:0]next_key"),
            ("state_rows[0:15]","state_rows[0:7]"),
            ("plru_rows[0:15]","plru_rows[0:7]"),
            ("reg[15:0]state_initialized,plru_initialized;","reg[7:0]state_initialized,plru_initialized;"),
            ("reg[5:0]pin_line;","reg[4:0]pin_line;"),
            ("reg[5:0]desc_line;","reg[4:0]desc_line;"),
            ("reg[5:0]hit_line,vic_line;","reg[4:0]hit_line,vic_line;"),
            ("wire[15:0]head_sector=head_payload[15:0];","wire[14:0]head_sector=head_payload[14:0];"),
            ("wire[2:0]head_tlx=head_payload[18:16],head_tly=head_payload[21:19];",
             "wire[3:0]head_tlx=head_payload[18:15];wire[2:0]head_tly=head_payload[21:19];"),
            ("wire[3:0]head_set=head_sector[3:0];","wire[2:0]head_set=head_sector[2:0];"),
            ("wire[31:0]hk_addr={9'd0,head_sector,7'd0};","wire[31:0]hk_addr={9'd0,head_sector,8'd0};"),
            ("wire y0=res_payload[19]^res_payload[17],y1=y0^res_payload[16];",
             "wire y0=res_payload[19]^res_payload[16],y1=y0^res_payload[15];"),
            ("physical_weights=res_payload[16]?","physical_weights=res_payload[15]?"),
            ("assign mem_req_addr=hk_addr;","assign mem_req_addr=hk_addr+{24'd0,desc_sector,7'd0};"),
            ("wire[17:0]head_tag={6'd0,head_sector[15:4]};","wire[17:0]head_tag={6'd0,head_sector[14:3]};"),
            ("wire[3:0]tag_set=(read_issue&&next_valid)?next_key[3:0]:head_set;",
             "wire[2:0]tag_set=(read_issue&&next_valid)?next_key[2:0]:head_set;"),
            ("{5'd0,vic_line[5:2],vic_line[0],4'b0011}","{6'd0,vic_line[4:2],vic_line[0],4'b0011}"),
            ("{5'd0,tag_set,5'd0}","{6'd0,tag_set,5'd0}"),
            ("tag_ram[0:63]","tag_ram[0:31]"),
            ("wire[3:0]state_set=desc_valid?desc_line[5:2]:head_set;",
             "wire[2:0]state_set=desc_valid?desc_line[4:2]:head_set;"),
            ("wire publish=terminal&&!reset&&!fault_eff&&term_ok;",
             "wire publish=terminal&&desc_sector&&!reset&&!fault_eff&&term_ok;"),
            ("wire state_write=tag_write||(desc_valid&&(terminal||fault_eff||(beat_present&&!beat_ok)));",
             "wire state_write=tag_write||(desc_valid&&((terminal&&(desc_sector||!term_ok))||fault_eff||(beat_present&&!beat_ok)));"),
            ("vic_line = 6'd0;","vic_line = 5'd0;"),
            ("wire[9:0]data_waddr={desc_line,mem_resp_index[3:1],mem_resp_index[0]};",
             "wire[9:0]data_waddr={desc_line,desc_sector,mem_resp_index};"),
            ("reg[4:0]desc_next;","reg[4:0]desc_next;reg desc_sector;"),
            ("desc_valid<=0;desc_started<=0;desc_presented<=0;desc_next<=0;",
             "desc_valid<=0;desc_started<=0;desc_presented<=0;desc_next<=0;desc_sector<=0;"),
            ("desc_line<=vic_line;desc_started<=0;","desc_line<=vic_line;desc_sector<=0;desc_started<=0;"),
            ("if(terminal)desc_valid<=0;",
             "// The parent line owns the head across both separately acknowledged sectors.\n   if(terminal)begin\n    if(!desc_sector&&term_ok&&!fault_eff)begin\n     desc_sector<=1;desc_started<=0;desc_presented<=0;desc_next<=0;\n    end else desc_valid<=0;\n   end"),
        ] {
            once(s,old,new)?;
        }
        // Same fixed bank rotation; the additional X bit extends only local
        // address carry. Paid-Q/read/Color holding and pin lifetimes stay intact.
        let begin = s
            .find(" wire[2:0]data_tlx=")
            .ok_or("Texture16x8 bank begin")?;
        let end = begin
            + s[begin..]
                .find(" wire data_write=")
                .ok_or("Texture16x8 bank end")?;
        let mut rows =
            " wire[3:0]data_tlx=paid_payload[18:15];wire[2:0]data_tly=paid_payload[21:19];\n"
                .to_owned();
        for bank in 0..4 {
            let high = bank >> 1;
            let inverse_low = 1 ^ (bank & 1);
            rows.push_str(&format!(
                " wire bank_low{bank}=1'b{high}^data_tlx[1]^(data_tlx[0]&1'b{inverse_low});\n wire bank_carry{bank}=data_tly[0]&!bank_low{bank};\n wire[4:0]bank_row{bank}={{data_tly[2]^(data_tly[1]&bank_carry{bank}),data_tly[1]^bank_carry{bank},bank_low{bank},data_tlx[3]^(data_tlx[2]&data_tlx[1]&data_tlx[0]&1'b{inverse_low}),data_tlx[2]^(data_tlx[1]&data_tlx[0]&1'b{inverse_low})}};\n"
            ));
        }
        s.replace_range(begin..end, &rows);
        *s = s.replace(
            "mem_resp_index[1]?mem_resp_data",
            "mem_resp_index[2]?mem_resp_data",
        );
        Ok(())
    })?;
    Ok(source)
}

/// Retain parent-line failure invalidation after the production diagnostic
/// specialization. A successful first sector alone never makes a line Ready.
pub fn protect_return_errors(mut source: String) -> Result<String, String> {
    edit(&mut source, "gpu_v2_texture_runtime_cache_unique", |s| {
        once(
            s,
            "wire publish=terminal&&desc_sector&&!reset&&!fault_eff&&term_ok;",
            "wire publish=terminal&&desc_sector&&mem_resp_ok&&!reset&&!fault_eff&&term_ok;",
        )?;
        once(
            s,
            "terminal&&(desc_sector||!term_ok)",
            "terminal&&(desc_sector||!term_ok||!mem_resp_ok)",
        )?;
        once(
            s,
            "if(!desc_sector&&term_ok&&!fault_eff)",
            "if(!desc_sector&&term_ok&&mem_resp_ok&&!fault_eff)",
        )
    })?;
    Ok(source)
}
