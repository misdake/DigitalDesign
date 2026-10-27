//! Ordered Final with one time-shared multiplier per color channel.
pub const MODULE: &str = "gpu_v2_final_ordered_shared";
pub const RESULT_CAPACITY: usize = 8;
/// Enabled edges from acceptance to result publication.
pub const LATENCY: usize = 3;
pub const INITIATION_INTERVAL: usize = 3;

/// Same flat port contract as ordered_compact. Captures every live input at
/// acceptance; no parent payload-stability loan is required after acceptance.
/// Native contains three unsigned MULT9X9 cells with registered outputs.
pub fn source(native: bool) -> String {
    let implementation = if native {
        r#"wire [17:0] raw;
 MULT9X9 #(.AREG(1'b0),.BREG(1'b0),.PIPE_REG(1'b0),.OUT_REG(1'b1),
 .ASIGN_REG(1'b0),.BSIGN_REG(1'b0),.SOA_REG(1'b0),.MULT_RESET_MODE("SYNC")) dsp(
 .DOUT(raw),.SOA(),.SOB(),.A(a),.B({1'b0,b}),.SIA(9'd0),.SIB(9'd0),
 .ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(ce),.RESET(1'b0));
 assign product=raw[16:0];"#
    } else {
        "reg [16:0] q; always @(posedge clk) if(ce) q<=a*b;\n assign product=q;"
    };
    include_str!("ordered_shared.v").replace("@MULTIPLIER@", implementation)
}

pub const SINGLE_MODULE: &str = "gpu_v2_final_ordered_single";

/// Closed parent candidate with exactly one reserved result, stored in FF.
/// When borrow_inputs is true the parent must preserve g/h/spec from actual
/// acceptance through the next two enabled edges (SPEC then BASE), including
/// wall-clock pauses. postquad reads the next Light row no earlier than the
/// third edge after acceptance and keeps draw specular immutable. Tint/texture
/// are consumed only at acceptance. The captured variant has no such loan.
/// The default shared factory and its eight-credit contract are unchanged.
pub fn source_single(native: bool, borrow_inputs: bool) -> String {
    let implementation = if native {
        r#"wire [17:0] raw;
 MULT9X9 #(.AREG(1'b0),.BREG(1'b0),.PIPE_REG(1'b0),.OUT_REG(1'b1),
 .ASIGN_REG(1'b0),.BSIGN_REG(1'b0),.SOA_REG(1'b0),.MULT_RESET_MODE("SYNC")) dsp(
 .DOUT(raw),.SOA(),.SOB(),.A(a),.B({1'b0,b}),.SIA(9'd0),.SIB(9'd0),
 .ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(ce),.RESET(1'b0));
 assign product=raw[16:0];"#
    } else {
        "reg [16:0] q; always @(posedge clk) if(ce) q<=a*b;\n assign product=q;"
    };
    include_str!("ordered_single.v")
        .replace("@MULTIPLIER@", implementation)
        .replace(
            "@INPUT_STORAGE@",
            if borrow_inputs {
                "// g/h/spec are the parent's immutable live loan."
            } else {
                "reg [8:0] g,h; reg [23:0] spec;"
            },
        )
        .replace("@G@", if borrow_inputs { "in_g" } else { "g" })
        .replace("@H@", if borrow_inputs { "in_h" } else { "h" })
        .replace("@SPEC@", if borrow_inputs { "in_spec" } else { "spec" })
        .replace(
            "@INPUT_CAPTURE@",
            if borrow_inputs {
                ""
            } else {
                "if(in_accept)begin g<=in_g;h<=in_h;spec<=in_spec;end"
            },
        )
}

pub const FORWARDED_MODULE: &str = "gpu_v2_final_ordered_single_forwarded";

/// Single-slot candidate offers the immutable PUBLISH product immediately.
/// On a stalled publication it captures RGB into the same fallback register;
/// on an accepted publication it leaves no duplicate queued result. The input
/// reservation remains old-state: publication/pop never funds same-edge input.
/// The optional input loan has exactly source_single's two-enabled-edge scope.
pub fn source_single_forwarded(native: bool, borrow_inputs: bool) -> String {
    let mut source = source_single(native, borrow_inputs);
    for (old, new) in [
        (
            "module gpu_v2_final_ordered_single(",
            "module gpu_v2_final_ordered_single_forwarded(",
        ),
        (
            "assign out_valid=!reset&&!fault&&result_valid;",
            "assign out_valid=!reset&&!fault&&(result_valid||(phase==PUBLISH));",
        ),
        (" wire [23:0] color;", ""),
        (
            "assign out_rgb=result;",
            "wire [23:0] color;\n assign out_rgb=(phase==PUBLISH)?color:result;",
        ),
        (
            "assign queued={3'b0,result_valid};",
            "assign queued={3'b0,(result_valid||(phase==PUBLISH))};",
        ),
        (
            "if(publish)begin result<=color;result_valid<=1;end",
            "if(publish)begin result<=color;result_valid<=!out_fire;end",
        ),
    ] {
        assert_eq!(
            source.matches(old).count(),
            1,
            "Final forwarding anchor: {old}"
        );
        source = source.replacen(old, new, 1);
    }
    source.replace("gpu_v2_final_single_mul9", "gpu_v2_final_forwarded_mul9")
}

/// One occupied result may transfer and reserve the next input on the same
/// enabled edge. Raw readiness depends on old phase and the downstream offer,
/// never on actual commit CE. PUBLISH consumes the old product before the
/// multiplier accepts the next tint/texture; the g/h loan still lasts two edges.
pub fn source_single_reissue(native: bool, borrow_inputs: bool) -> String {
    let mut source = source_single_forwarded(native, borrow_inputs);
    for (old, new) in [
        ("module gpu_v2_final_ordered_single_forwarded(", "module gpu_v2_final_ordered_single_reissue("),
        ("output wire in_ready,in_accept,advance,preflight_bad,fault,", "output wire in_ready,proposal_ready,in_accept,advance,preflight_bad,fault,"),
        ("assign in_ready=running&&!occupied;", "assign proposal_ready=!reset&&!fault&&(!occupied||((phase==IDLE||phase==PUBLISH)&&out_ready));\n assign in_ready=running&&proposal_ready;"),
        ("assign in_accept=advance&&in_valid&&!occupied;", "assign in_accept=advance&&in_valid&&proposal_ready;"),
        ("PUBLISH:phase<=IDLE;", "PUBLISH:if(in_accept)phase<=SPEC;else phase<=IDLE;"),
    ] {
        assert_eq!(source.matches(old).count(),1,"Final reissue anchor: {old}");
        source=source.replacen(old,new,1);
    }
    source.replace("gpu_v2_final_forwarded_mul9", "gpu_v2_final_reissue_mul9")
}

/// Six small products implement the exact full recipe at pixel II2. Acceptance
/// pays tint*texture and specular*h together; the following enabled edge pays
/// rounded_base*g. RGB can transfer while the next input is accepted. The
/// borrowed g is live through that one following edge, h/spec only at acceptance.
pub fn source_two_cycle(native: bool, borrow_inputs: bool) -> String {
    let mut source = source_single_reissue(native, borrow_inputs);
    let g = if borrow_inputs { "in_g" } else { "g" };
    let h = if borrow_inputs { "in_h" } else { "h" };
    let spec = if borrow_inputs { "in_spec" } else { "spec" };
    let spec_operand = format!("SPEC:begin a={h};b={spec}[c*8+:8];end");
    let base_operand = format!("BASE:begin a={g};b=base;end");
    let multiplier="gpu_v2_final_reissue_mul9 multiplier(.clk(clk),.ce(mul_ce),.a(a),.b(b),.product(product));";
    let spec_multiplier=format!("{multiplier}\n  wire [16:0] spec_product;\n  gpu_v2_final_reissue_mul9 spec_multiplier(.clk(clk),.ce(advance&&in_accept),.a(in_h),.b(in_spec[c*8+:8]),.product(spec_product));");
    for (old, new) in [
        (
            "module gpu_v2_final_ordered_single_reissue(".to_owned(),
            "module gpu_v2_final_ordered_two_cycle(".to_owned(),
        ),
        (spec_operand, format!("SPEC:begin a={g};b=folded[15:8];end")),
        (base_operand, String::new()),
        ("reg [7:0] base;".to_owned(), String::new()),
        (
            "if(phase==SPEC)base<=folded[15:8];".to_owned(),
            String::new(),
        ),
        (
            "if(phase==BASE)sh<=product[15:0];".to_owned(),
            "if(phase==SPEC)sh<=spec_product[15:0];".to_owned(),
        ),
        (
            "wire mul_ce=advance&&(in_accept||(phase==SPEC)||(phase==BASE));".to_owned(),
            "wire mul_ce=advance&&(in_accept||(phase==SPEC));".to_owned(),
        ),
        (multiplier.to_owned(), spec_multiplier),
        (
            "BASE:phase<=PUBLISH;\n     PUBLISH:if(in_accept)phase<=SPEC;else phase<=IDLE;"
                .to_owned(),
            "BASE:if(in_accept)phase<=SPEC;else phase<=IDLE;".to_owned(),
        ),
    ] {
        assert_eq!(
            source.matches(&old).count(),
            1,
            "Two-cycle Final anchor: {old}"
        );
        source = source.replacen(&old, &new, 1);
    }
    source = source.replace("phase==PUBLISH", "phase==BASE");
    if !borrow_inputs {
        source = source.replace("reg [8:0] g,h; reg [23:0] spec;", "reg [8:0] g;");
        source = source.replace(
            "if(in_accept)begin g<=in_g;h<=in_h;spec<=in_spec;end",
            "if(in_accept)g<=in_g;",
        );
    }
    source.replace("gpu_v2_final_reissue_mul9", "gpu_v2_final_two_cycle_mul9")
}

/// Perceptual-width experiment: retain eight integer color bits and zero to
/// two fractional bits per high-light channel. Both nonnegative terms are
/// reduced before the fabric sum; two integer overflow bits preserve output
/// saturation when g exceeds unity. The physical six-multiplier II2 calendar
/// and parent input loan are unchanged. The exact factory remains a reference.
pub fn source_two_cycle_narrow(native: bool, borrow_inputs: bool, fractional_bits: u8) -> String {
    assert!(fractional_bits <= 2);
    let mut source = source_two_cycle(native, borrow_inputs);
    let shift = 8 - fractional_bits;
    let sum_top = 9 + fractional_bits;
    let round_up = match fractional_bits {
        0 => "1'b0",
        1 => "sum[0]&&sum[1]",
        2 => "sum[1]&&(sum[0]||sum[2])",
        _ => unreachable!(),
    };
    for (old, new) in [
        (
            "reg [15:0] sh;".to_owned(),
            format!("reg [{}:0] sh;", 7 + fractional_bits),
        ),
        (
            "if(phase==SPEC)sh<=spec_product[15:0];".to_owned(),
            format!("if(phase==SPEC)sh<=spec_product[15:{shift}];"),
        ),
        (
            "wire [17:0] sum={1'b0,product}+{2'b0,sh};".to_owned(),
            format!("wire [{sum_top}:0] sum={{1'b0,product[16:{shift}]}}+{{2'b0,sh}};"),
        ),
        (
            "wire round_up=sum[7]&&((|sum[6:0])||sum[8]);".to_owned(),
            format!("wire round_up={round_up};"),
        ),
        (
            "wire saturated=(|sum[17:16])||(&sum[15:8]);".to_owned(),
            format!(
                "wire saturated=(|sum[{sum_top}:{}])||(&sum[{}:{fractional_bits}]);",
                8 + fractional_bits,
                7 + fractional_bits
            ),
        ),
        (
            "wire [7:0] rounded=sum[15:8]+{7'd0,round_up};".to_owned(),
            format!(
                "wire [7:0] rounded=sum[{}:{fractional_bits}]+{{7'd0,round_up}};",
                7 + fractional_bits
            ),
        ),
    ] {
        assert_eq!(
            source.matches(&old).count(),
            1,
            "Narrow Final anchor: {old}"
        );
        source = source.replacen(&old, &new, 1);
    }
    source
}
