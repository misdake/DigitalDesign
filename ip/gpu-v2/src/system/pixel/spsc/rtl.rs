//! Synthesizable RAM, publication pointers, synchronous return and two heads.

use super::{Config, ReadTiming};

pub const MODULE: &str = "gpu_v2_published_spsc";

pub fn source(config: Config) -> Result<String, String> {
    config.validate()?;
    let prefix = format!(
        "module {MODULE} #(parameter DEPTH={}, ROWS={}, WIDTH={}, SYNC={})(\n",
        config.entries,
        config.rows,
        config.width,
        usize::from(config.read_timing == ReadTiming::Registered)
    );
    let text = prefix + BODY;
    Ok(if config.read_timing == ReadTiming::Capture {
        text.replace(
            "  reg [WIDTH-1:0] ram",
            "  (* syn_ramstyle = \"distributed_ram\" *) reg [WIDTH-1:0] ram",
        )
    } else {
        text
    })
}

/// Explicit single SDP36 block. The 32 useful Sampling rows occupy a prefix
/// of its native 512x36 array. READ_MODE=0 exposes the synchronous hard BP/DO;
/// it replaces the behavioral pending-data register, not an extra payload copy.
pub fn source_sdp36(config: Config) -> Result<String, String> {
    if config.width != 36
        || config.read_timing != ReadTiming::Registered
        || config.entries * config.rows > 512
    {
        return Err(
            "SDP36 adapter requires registered 36-bit rows fitting one native block".into(),
        );
    }
    let text = source(config)?
        .replace(
            "  reg [WIDTH-1:0] ram [0:DEPTH*ROWS-1];",
            "  // Payload resides exclusively in the SDPX9B below.",
        )
        .replace(
            "  reg [WIDTH-1:0] pending_data;",
            "  wire [35:0] pending_data;",
        )
        .replace("        ram[write_address] <= in_data;\n", "")
        .replace(
            "        head_data[head_write] <= SYNC ? pending_data : ram[read_address];",
            "        head_data[head_write] <= pending_data;",
        )
        .replace("          pending_data <= ram[read_address];\n", "");
    if text.contains("ram[") {
        return Err("SDP36 control template retains behavioral payload RAM".into());
    }
    let primitive = r#"
  SDPX9B #(.READ_MODE(1'b0), .BIT_WIDTH_0(36), .BIT_WIDTH_1(36),
    .BLK_SEL_0(3'b000), .BLK_SEL_1(3'b000), .RESET_MODE("SYNC")) payload(
    .DO(pending_data), .DI(in_data),
    .BLKSELA(3'b000), .BLKSELB(3'b000),
    .ADA({write_address[8:0],1'b0,4'b1111}),
    .ADB({read_address[8:0],5'b00000}),
    .CLKA(clk), .CLKB(clk), .CEA(push), .CEB(read_request),
    .OCE(ce), .RESETA(1'b0), .RESETB(reset));
"#;
    Ok(text.replace(
        "  always @(posedge clk) begin",
        &(primitive.to_string() + "\n  always @(posedge clk) begin"),
    ))
}

const BODY: &str = r#"  input wire clk, reset, ce,
  input wire in_valid,
  input wire [WIDTH-1:0] in_data,
  output wire in_ready,
  input wire out_ready,
  output wire out_valid,
  output wire [WIDTH-1:0] out_data,
  output wire [3:0] out_row,
  output wire out_last,
  output wire published,
  output wire read_request,
  output wire [9:0] read_address
);
  localparam PW = $clog2(DEPTH) + 1;
  // No RAM reset loop: every row is written before insert publishes its entry.
  reg [WIDTH-1:0] ram [0:DEPTH*ROWS-1];
  reg [PW-1:0] insert_ptr, consume_ptr, issue_ptr;
  reg [3:0] write_row, issue_row;
  wire [PW-1:0] used = insert_ptr - consume_ptr;
  reg [WIDTH-1:0] pending_data;
  reg [3:0] pending_row;
  reg pending_valid;
  reg [WIDTH-1:0] head_data [0:1];
  reg [3:0] head_row [0:1];
  reg head_read, head_write;
  reg [1:0] head_count;

  assign in_ready = !reset && ce && ((write_row != 0) || used < DEPTH);
  assign out_valid = !reset && (head_count != 0);
  assign out_data = head_data[head_read];
  assign out_row = head_row[head_read];
  assign out_last = out_row == ROWS-1;
  wire push = in_valid && in_ready;
  wire pop = ce && out_valid && out_ready;
  wire returned = SYNC ? (!reset && ce && pending_valid && head_count < 2) : read_request;
  assign published = push && write_row == ROWS-1;
  // A full pair of heads cannot borrow a position popped on this edge.
  // Earlier rows are stored before the final row's publication edge. Capture
  // one at that edge while writing a different address; expose it next-edge.
  // A one-row entry cannot use this path because read/write would collide.
  assign read_request = !reset && ce && head_count < 2 &&
    ((issue_ptr != insert_ptr) || (published && ROWS > 1 && issue_row < write_row));
  assign read_address = (issue_ptr % DEPTH) * ROWS + issue_row;
  wire [9:0] write_address = (insert_ptr % DEPTH) * ROWS + write_row;

  always @(posedge clk) begin
    if (reset) begin
      insert_ptr <= 0; consume_ptr <= 0; issue_ptr <= 0;
      write_row <= 0; issue_row <= 0; pending_valid <= 0;
      head_read <= 0; head_write <= 0; head_count <= 0;
    end else if (ce) begin
      if (push) begin
        ram[write_address] <= in_data;
        if (published) begin insert_ptr <= insert_ptr + 1'b1; write_row <= 0; end
        else write_row <= write_row + 1'b1;
      end
      if (pop) begin
        head_read <= !head_read;
        if (out_last) consume_ptr <= consume_ptr + 1'b1;
      end
      if (returned) begin
        head_data[head_write] <= SYNC ? pending_data : ram[read_address];
        head_row[head_write] <= SYNC ? pending_row : issue_row;
        head_write <= !head_write;
      end
      case ({returned,pop})
        2'b10: head_count <= head_count + 1'b1;
        2'b01: head_count <= head_count - 1'b1;
        default: head_count <= head_count;
      endcase
      if (read_request) begin
        if (SYNC) begin
          pending_data <= ram[read_address];
          pending_row <= issue_row;
          pending_valid <= 1;
        end
        if (issue_row == ROWS-1) begin issue_ptr <= issue_ptr + 1'b1; issue_row <= 0; end
        else issue_row <= issue_row + 1'b1;
      end else if (returned) pending_valid <= 0;
    end
  end
endmodule
"#;
