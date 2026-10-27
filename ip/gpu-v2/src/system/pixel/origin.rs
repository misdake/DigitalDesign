//! Ordered quad origin storage. Only ROP reads the aligned 15-bit coordinates.
//! A current origin survives row7 stalls; the paid RAM DO owns the next origin.
//! The combined foundation is not RTL: this is its independently replayed leaf.

pub const MODULE: &str = "gpu_v2_pixel_origin";
pub const ROWS: usize = 32;

pub fn pack(x: u16, y: u8) -> Result<u16, String> {
    if x > 510 || x & 1 != 0 || y > 254 || y & 1 != 0 {
        return Err("origin range/alignment".into());
    }
    Ok((x >> 1) | (u16::from(y >> 1) << 8))
}

pub fn unpack(data: u16) -> (u16, u8) {
    ((data & 255) << 1, ((data >> 8) as u8) << 1)
}

#[derive(Clone, Copy, Default)]
pub struct Tick {
    pub reset: bool,
    pub ce: bool,
    pub write: Option<(u8, u16)>,
    /// Only Output's row-zero read request creates an origin reservation.
    pub read: Option<u8>,
    /// Actual Output row7 consumption, not publication or speculative issue.
    pub retire: bool,
}

pub struct Step {
    pub output: Option<u16>,
    pub read: Option<u8>,
    pub write: Option<(u8, u16)>,
    pub returned: Option<(u8, u16, u64)>,
}

pub struct Model {
    rows: [u16; ROWS],
    current: Option<u16>,
    // Models hard synchronous DO plus its valid bit, not a soft RAM shadow.
    pending: Option<u16>,
    // Host-only port/edge witness for the previous read.
    flight: Option<(u8, u64)>,
    wall: u64,
    max_wall: u64,
}

impl Model {
    pub fn new(max_wall: u64) -> Self {
        Self {
            rows: std::array::from_fn(|i| (0x56ab ^ i as u16) & 0x7fff),
            current: None,
            pending: None,
            flight: None,
            wall: 0,
            max_wall,
        }
    }

    pub fn output(&self) -> Option<u16> {
        self.current.or(self.pending)
    }

    pub fn read_ready(&self, ce: bool) -> bool {
        ce && self.pending.is_none()
    }

    pub fn idle(&self) -> bool {
        self.current.is_none() && self.pending.is_none() && self.flight.is_none()
    }

    pub fn tick(&mut self, tick: Tick) -> Result<Step, String> {
        if self.wall >= self.max_wall {
            return Err("origin wall watchdog".into());
        }
        let output = (!tick.reset).then(|| self.output()).flatten();
        let read = tick.read.filter(|_| tick.ce && !tick.reset);
        let write = tick.write.filter(|_| tick.ce && !tick.reset);
        if read.is_some_and(|a| usize::from(a) >= ROWS)
            || write.is_some_and(|(a, d)| usize::from(a) >= ROWS || d > 0x7fff)
            || (read.is_some() && self.pending.is_some())
            || (tick.ce && !tick.reset && tick.retire && output.is_none())
            || read.is_some_and(|a| write.is_some_and(|(b, _)| a == b))
        {
            return Err("origin address/credit/retirement/collision".into());
        }
        self.wall += 1;
        // The prior read is visible before this edge, including CE=0. Its
        // payload cannot be overwritten until after the old-state observation.
        let returned = self
            .flight
            .take()
            .map(|(address, issued)| (address, self.pending.unwrap(), issued));
        if tick.reset {
            self.current = None;
            self.pending = None;
        } else if tick.ce {
            if tick.retire {
                if self.current.is_some() {
                    self.current = self.pending.take();
                } else {
                    self.pending = None;
                }
            } else if self.current.is_none() {
                self.current = self.pending.take();
            }
            if let Some(address) = read {
                self.pending = Some(self.rows[usize::from(address)]);
                self.flight = Some((address, self.wall));
            }
            if let Some((address, data)) = write {
                self.rows[usize::from(address)] = data;
            }
        }
        Ok(Step {
            output,
            read,
            write,
            returned,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Backend {
    Native,
    Distributed,
    /// Matched old Status-origin lowering; only the consumer slot reads data.
    AsyncReference,
}

pub fn verilog(backend: Backend) -> String {
    let mut source = String::from(BODY);
    match backend {
        Backend::Native => {
            source = source.replace("  (* syn_ramstyle = \"distributed_ram\" *) reg [14:0] ram [0:31];", "  wire [35:0] native_data;")
                .replace("  reg [14:0] pending_data;", "  wire [14:0] pending_data = native_data[14:0];")
                .replace("      if (write_en) ram[write_address] <= write_data;\n", "")
                .replace("      if (read_en) begin pending_data <= ram[read_address]; pending_valid <= 1; end", "      if (read_en) pending_valid <= 1;");
            let primitive = r#"
  SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),
    .BLK_SEL_0(3'b000),.BLK_SEL_1(3'b000),.RESET_MODE("SYNC")) payload(
    .DO(native_data),.DI({21'd0,write_data}),.BLKSELA(3'b000),.BLKSELB(3'b000),
    .ADA({4'd0,write_address,1'b0,4'b1111}),.ADB({4'd0,read_address,5'b00000}),
    .CLKA(clk),.CLKB(clk),.CEA(write_en),.CEB(read_en),.OCE(1'b1),
    .RESETA(1'b0),.RESETB(reset));
"#;
            source = source.replace("endmodule", &(primitive.to_owned() + "endmodule"));
            assert!(!source.contains("ram["));
        }
        Backend::Distributed => {}
        Backend::AsyncReference => {
            source = source.replace(
                "assign origin_data = current_valid ? current_data : pending_data;",
                "assign origin_data = ram[current_address];",
            );
        }
    }
    source
}

const BODY: &str = r#"module gpu_v2_pixel_origin(
  input wire clk,reset,ce,write_valid,read_valid,retire,
  input wire [4:0] write_address,read_address,current_address,
  input wire [14:0] write_data,
  output wire read_ready,origin_valid,collision,
  output wire [14:0] origin_data
);
  (* syn_ramstyle = "distributed_ram" *) reg [14:0] ram [0:31];
  reg [14:0] current_data;
  reg [14:0] pending_data;
  reg current_valid,pending_valid;
  assign read_ready = !reset && ce && !pending_valid;
  wire read_en = read_valid && read_ready && !collision;
  wire write_en = !reset && ce && write_valid && !collision;
  assign collision = !reset && ce && write_valid && read_valid && !pending_valid && write_address == read_address;
  assign origin_valid = !reset && (current_valid || pending_valid);
  assign origin_data = current_valid ? current_data : pending_data;
  always @(posedge clk) begin
    if(reset) begin current_valid<=0;pending_valid<=0;end
    else if(ce) begin
      if(retire) begin
        if(current_valid) begin
          current_valid<=pending_valid;
          if(pending_valid) current_data<=pending_data;
          pending_valid<=0;
        end else pending_valid<=0;
      end else if(!current_valid && pending_valid) begin
        current_data<=pending_data;current_valid<=1;pending_valid<=0;
      end
      if (read_en) begin pending_data <= ram[read_address]; pending_valid <= 1; end
      if (write_en) ram[write_address] <= write_data;
    end
  end
endmodule
"#;
