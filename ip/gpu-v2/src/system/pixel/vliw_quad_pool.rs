//! Direct I-result pool: three independent payload banks and dense tombstones.
//!
//! Front CE qualifies the fixed I return calendar; backend CE qualifies publish,
//! consumers and retirement. Unpublished credit8 protects the producer's low3
//! tags; total credit24 spans the former front8 plus back16, on32 physical slots.
//! Empty slots remain dense and require a safe backend issue/retire skip event.
//! The pool does not authorize that backend skip merely by detecting mask0.

use crate::lighting::ports::pixel_center_ndc;
use std::fmt::Write;

pub const MODULE: &str = "gpu_v2_vliw_quad_pool";
pub const SOURCE: &str = include_str!("vliw_quad_pool.v");

/// Adjacent X pixel centers in rows0..199, adjacent Y centers in200..319.
/// Y is up-positive, exactly the main Lighting input contract, with one RNE.
pub fn ndc_words() -> [u32; 320] {
    std::array::from_fn(|row| {
        let (a, b) = if row < 200 {
            (
                pixel_center_ndc((row * 2) as u16, 0, 400, 240).unwrap()[0],
                pixel_center_ndc((row * 2 + 1) as u16, 0, 400, 240).unwrap()[0],
            )
        } else {
            (
                pixel_center_ndc(0, ((row - 200) * 2) as u16, 400, 240).unwrap()[1],
                pixel_center_ndc(0, ((row - 200) * 2 + 1) as u16, 400, 240).unwrap()[1],
            )
        };
        u32::from(a as u16) | (u32::from(b as u16) << 16)
    })
}

pub fn verilog(portable: bool) -> String {
    let mut s = if portable {
        "`define GPU_VLIW_POOL_PORTABLE\n".to_owned()
    } else {
        String::new()
    };
    s.push_str(SOURCE);
    s.push_str("\nmodule gpu_v2_vliw_ndc_rom(input clk,reset,re,input[8:0]ra,output[31:0]q);\n");
    let rows = ndc_words();
    if portable {
        s.push_str("reg[31:0]memory[0:511];reg[31:0]held;assign q=held;initial begin\n");
        for (i, word) in rows.into_iter().enumerate() {
            writeln!(s, "memory[{i}]=32'h{word:08x};").unwrap();
        }
        s.push_str("end\nalways @(posedge clk)if(reset)held<=0;else if(re)held<=memory[ra];\n");
    } else {
        s.push_str("wire[35:0]raw;assign q=raw[31:0];\npROMX9 #(.BIT_WIDTH(36),.READ_MODE(0),.RESET_MODE(\"SYNC\")");
        for chunk in 0..64 {
            write!(s, ",\n.INIT_RAM_{chunk:02X}(288'h").unwrap();
            for lane in (0..8).rev() {
                write!(
                    s,
                    "{:09x}",
                    rows.get(chunk * 8 + lane).copied().unwrap_or(0)
                )
                .unwrap();
            }
            s.push(')');
        }
        s.push_str(") rom(.DO(raw),.AD({ra,5'd0}),.CLK(clk),.CE(re),.OCE(1'b1),.RESET(reset));\n");
    }
    s.push_str("endmodule\n");
    s
}
