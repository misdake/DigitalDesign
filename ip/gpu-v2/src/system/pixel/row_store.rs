//! One read and one write port with a reserved, one-wall-edge read return.
//! No reset clears payload. Callers publish only fully written rows and reject
//! same-address read/write; the independent RTL follows the same port contract.

/// One authoritative layout for the actual foundation model and its native
/// three-bank implementation. Each bank has independent 1R1W ports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    pub rows: usize,
    pub width: u32,
}

pub const FOUNDATION_SHAPES: [Shape; 3] = [
    Shape {
        rows: super::foundation::GLOBAL_SLOTS * 8,
        width: 24,
    },
    Shape {
        rows: super::foundation::GLOBAL_SLOTS * 4,
        width: 18,
    },
    Shape {
        rows: super::foundation::GLOBAL_SLOTS * 4,
        width: 24,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Return {
    pub address: usize,
    pub data: u32,
    pub issued: u64,
}

pub struct Bank {
    rows: Vec<u32>,
    mask: u32,
    pending: Option<Return>,
    wall: u64,
}

impl Bank {
    pub fn with_shape(shape: Shape) -> Result<Self, String> {
        Self::new(shape.rows, shape.width)
    }

    pub fn new(rows: usize, width: u32) -> Result<Self, String> {
        if rows == 0 || !rows.is_power_of_two() || !(1..=32).contains(&width) {
            return Err("row store shape".into());
        }
        let mask = u32::MAX >> (32 - width);
        Ok(Self {
            rows: (0..rows).map(|r| (0xabad_5678 ^ r as u32) & mask).collect(),
            mask,
            pending: None,
            wall: 0,
        })
    }

    /// The old pending return is captured even while the caller's compute CE
    /// is low. Its reserved destination must remain occupied until consumption.
    pub fn tick(
        &mut self,
        read: Option<usize>,
        write: Option<(usize, u32)>,
    ) -> Result<Option<Return>, String> {
        if read.is_some_and(|r| r >= self.rows.len())
            || write.is_some_and(|(r, d)| r >= self.rows.len() || d & !self.mask != 0)
        {
            return Err("row store address/width".into());
        }
        if read.is_some() && read == write.map(|(r, _)| r) {
            return Err("row store same-address read/write collision".into());
        }
        self.wall += 1;
        let returned = self.pending.take();
        if let Some(r) = returned {
            assert_eq!(r.issued + 1, self.wall);
        }
        self.pending = read.map(|address| Return {
            address,
            data: self.rows[address],
            issued: self.wall,
        });
        if let Some((address, data)) = write {
            self.rows[address] = data;
        }
        Ok(returned)
    }
}

/// Matching behavioral 1R1W RTL. This is a bank leaf, not a pixel-controller
/// implementation or proof of RAM inference/PnR.
pub fn verilog(rows: usize, width: u32) -> Result<String, String> {
    Bank::new(rows, width)?;
    let address_bits = rows.ilog2().max(1);
    Ok(format!(
        r#"module gpu_v2_pixel_row_store(
 input wire clk,reset,read_en,write_en,
 input wire [{ab}:0] read_address,write_address,
 input wire [{db}:0] write_data,
 output reg return_valid,
 output reg [{db}:0] return_data,
 output wire collision);
 reg [{db}:0] ram[0:{last}];
 assign collision=read_en && write_en && read_address==write_address;
 always @(posedge clk) begin
  if(reset) return_valid<=1'b0;
  else begin
   return_valid<=read_en && !collision;
   if(read_en && !collision) return_data<=ram[read_address];
   if(write_en && !collision) ram[write_address]<=write_data;
  end
 end
endmodule
"#,
        ab = address_bits - 1,
        db = width - 1,
        last = rows - 1
    ))
}

/// Explicit native SDP36 mapping; useful rows occupy only a prefix. The full
/// block is charged even for narrow payloads. Hard DO provides the read bank,
/// with no additional behavioral payload array or return-data register.
pub fn verilog_sdp36(rows: usize, width: u32) -> Result<String, String> {
    if rows > 512 {
        return Err("row store does not fit one SDP36 block".into());
    }
    let mut source = verilog(rows, width)?;
    let address_bits = rows.ilog2().max(1);
    let data = format!("{{{}'d0,write_data}}", 36 - width);
    let prefix = if address_bits == 9 {
        String::new()
    } else {
        format!("{}'d0,", 9 - address_bits)
    };
    let wa = format!("{{{prefix}write_address,1'b0,4'b1111}}");
    let ra = format!("{{{prefix}read_address,5'b00000}}");
    source = source
        .replace("output reg [", "output wire [")
        .replace(
            &format!(" reg [{}:0] ram[0:{}];", width - 1, rows - 1),
            " wire [35:0] primitive_data;",
        )
        .replace(
            "   if(read_en && !collision) return_data<=ram[read_address];\n",
            "",
        )
        .replace(
            "   if(write_en && !collision) ram[write_address]<=write_data;\n",
            "",
        );
    let primitive = format!(
        r#"
 assign return_data=primitive_data[{db}:0];
 SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),
 .BLK_SEL_0(3'b000),.BLK_SEL_1(3'b000),.RESET_MODE("SYNC")) payload(
 .DO(primitive_data),.DI({data}),
 .BLKSELA(3'b000),.BLKSELB(3'b000),
 .ADA({wa}),
 .ADB({ra}),
 .CLKA(clk),.CLKB(clk),.CEA(write_en && !collision && !reset),
 .CEB(read_en && !collision && !reset),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
"#,
        db = width - 1,
        data = data,
        wa = wa,
        ra = ra
    );
    if source.contains("ram[") {
        return Err("SDP36 template retained a behavioral payload array".into());
    }
    Ok(source.replace("endmodule", &(primitive + "endmodule")))
}

/// Native bank set used by the foundation's Basic, Light and Sample calendar.
/// Hard DO supplies each returned payload; only valid bits use soft registers.
/// This implements the banks, not the surrounding status/join controller.
pub fn verilog_foundation_banks() -> Result<String, String> {
    let mut leaves = String::new();
    let mut ports = vec!["input wire clk,reset".to_owned()];
    let mut instances = String::new();
    for (name, shape) in ["basic", "light", "sample"]
        .into_iter()
        .zip(FOUNDATION_SHAPES)
    {
        let module = format!("gpu_v2_pixel_{name}_store");
        leaves.push_str(
            &verilog_sdp36(shape.rows, shape.width)?.replace("gpu_v2_pixel_row_store", &module),
        );
        let ab = shape.rows.ilog2() - 1;
        let db = shape.width - 1;
        ports.push(format!("input wire {name}_read_en,{name}_write_en"));
        ports.push(format!(
            "input wire [{ab}:0] {name}_read_address,{name}_write_address"
        ));
        ports.push(format!("input wire [{db}:0] {name}_write_data"));
        ports.push(format!("output wire {name}_return_valid,{name}_collision"));
        ports.push(format!("output wire [{db}:0] {name}_return_data"));
        let connections = [
            "read_en",
            "write_en",
            "read_address",
            "write_address",
            "write_data",
            "return_valid",
            "collision",
            "return_data",
        ]
        .map(|pin| format!(".{pin}({name}_{pin})"))
        .join(",");
        instances.push_str(&format!(
            " {module} {name}(.clk(clk),.reset(reset),{connections});\n"
        ));
    }
    Ok(format!(
        "{leaves}\nmodule gpu_v2_pixel_foundation_banks(\n{});\n{instances}endmodule\n",
        ports.join(",\n")
    ))
}
