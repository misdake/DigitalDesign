//! Ordered, tagless Final candidates. The parent owns destination identity.
pub const MODULE: &str = "gpu_v2_final_ordered_compact";
pub const RESULT_CAPACITY: usize = 8;
/// Enabled edges from input acceptance to result RAM publication.
pub const LATENCY: usize = 4;
pub const INITIATION_INTERVAL: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// Two bounded /255 correction adds in one stage.
    Folded,
    /// Exact high+low digit sum and thresholds 128/383.
    Threshold,
}

/// Same ports, credits and timing for both static arithmetic profiles.
/// Byte zero of each packed RGB bus is red. Native uses nine MULT9X9 cells
/// with output registers; all additions and saturation remain in fabric.
pub fn source(native: bool, profile: Profile) -> String {
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
    include_str!("ordered_compact.v")
        .replace(
            "@BASE_THRESHOLDS@",
            if profile == Profile::Threshold {
                "1"
            } else {
                "0"
            },
        )
        .replace("@MULTIPLIER@", implementation)
}
