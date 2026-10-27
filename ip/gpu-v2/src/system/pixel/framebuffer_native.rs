//! Explicit native ROP products, with the existing stage-three timing.
//! Avoid an inferred MAC cascade cycle in Gowin; all sums remain in fabric.
pub fn bind_products(mut source: String) -> String {
    let declaration = "    reg [15:0] psr3, pdr3, psg3, pdg3, psb3, pdb3;";
    assert_eq!(source.matches(declaration).count(), 1);
    source = source.replacen(
        declaration,
        "    wire [15:0] psr3, pdr3, psg3, pdg3, psb3, pdb3;",
        1,
    );
    let mut instances = String::new();
    for (name, a, b) in [
        ("psr3", "a2", "sr2"),
        ("pdr3", "ia2", "dr2"),
        ("psg3", "a2", "sg2"),
        ("pdg3", "ia2", "dg2"),
        ("psb3", "a2", "sb2"),
        ("pdb3", "ia2", "db2"),
    ] {
        let statement = format!("            {name} <= {{8'd0, {a}}} * {{8'd0, {b}}};\n");
        assert_eq!(source.matches(&statement).count(), 1);
        source = source.replacen(&statement, "", 1);
        instances.push_str(&format!(
            "gpu_v2_rop_mul9 {name}_binding(.clk(clk),.ce(ce),.a({a}),.b({b}),.product({name}));\n"
        ));
    }
    // Only the last module in this source is the ROP leaf.
    let end = source.rfind("endmodule").expect("ROP module end");
    source.insert_str(end, &instances);
    source.push_str(
        r#"
module gpu_v2_rop_mul9(input wire clk,ce,input wire[7:0]a,b,output wire[15:0]product);
 wire[17:0]raw;
 MULT9X9 #(.AREG(1'b0),.BREG(1'b0),.PIPE_REG(1'b0),.OUT_REG(1'b1),
 .ASIGN_REG(1'b0),.BSIGN_REG(1'b0),.SOA_REG(1'b0),.MULT_RESET_MODE("SYNC")) dsp(
 .DOUT(raw),.SOA(),.SOB(),.A({1'b0,a}),.B({1'b0,b}),.SIA(9'd0),.SIB(9'd0),
 .ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(ce),.RESET(1'b0));
 assign product=raw[15:0];
endmodule
"#,
    );
    source
}
