"""Functional board-reference clock/pad corner; final SDF qualification is separate."""
import re

def with_board_pins(tb, controller):
    # Current 108 MHz launch/forwarded clocks, with a fixed routed pad corner.
    # Input templates already use the sole stream contract, not an old default.
    tb=tb.replace('controller_clk=0','source_clk=0')
    original='always #4.629629 controller_clk=~controller_clk;'
    assert tb.count(original)==1, 'expected the current 108 MHz clock template'
    tb=tb.replace(original, 'wire controller_clk;assign #(2.271) controller_clk=source_clk;\nalways #4.629629 source_clk=~source_clk;')
    tb=tb.replace('.RETURN_MIN(1),.RETURN_MAX(4)', '.RETURN_MIN(1.392),.RETURN_MAX(2.314)')
    pads=f"""wire pin_cke,pin_cs,pin_ras,pin_cas,pin_we,pin_write_drive;
wire[3:0]pin_dqm;wire[10:0]pin_a;wire[1:0]pin_ba;wire[31:0]pin_dq,pin_output_data;
assign #(3.916) {{pin_cke,pin_cs,pin_ras,pin_cas,pin_we,pin_dqm,pin_a,pin_ba}}={{O_sdram_cke,O_sdram_cs_n,O_sdram_ras_n,O_sdram_cas_n,O_sdram_wen_n,O_sdram_dqm,O_sdram_addr,O_sdram_ba}};
assign #(2.300) pin_write_drive={controller}.dq_drive;
assign #(3.916) pin_output_data={controller}.dq_out;
assign pin_dq=pin_write_drive?pin_output_data:32'bz;
assign IO_sdram_dq={controller}.dq_drive?32'bz:pin_dq;
real command_change=-1e6,pin_edge=-1e6,dq_change=-1e6,read_change=-1e6,read_edge=-1e6;
wire[21:0]pin_command={{pin_cke,pin_cs,pin_ras,pin_cas,pin_we,pin_dqm,pin_a,pin_ba}};
always @(pin_command)begin
 if({controller}.initialized&&!reset&&$realtime-pin_edge<0.999)$fatal(1,"physical command/DQM hold");
 command_change=$realtime;
end
always @(pin_dq)begin
 if(pin_write_drive&&{controller}.initialized&&!reset&&pin.writing_bus&&pin_dqm!=15&&$realtime-pin_edge<0.999)$fatal(1,"physical write DQ hold");
 dq_change=$realtime;
end
always @(posedge O_sdram_clk)begin
 if({controller}.initialized&&!reset)begin
  if($realtime-command_change<1.499)$fatal(1,"physical command/DQM setup");
  if(pin_write_drive&&pin_dqm!=15&&$realtime-dq_change<1.499)$fatal(1,"physical write DQ setup");
 end
 pin_edge=$realtime;
end
always @(IO_sdram_dq)begin
 if({controller}.initialized&&!reset&&$realtime-read_edge<0.010)$fatal(1,"physical read input hold");
 read_change=$realtime;
end
always @(posedge controller_clk)if({controller}.capture_valid&&!reset)begin
 if($realtime-read_change<0.034||$isunknown(IO_sdram_dq))$fatal(1,"physical read input setup");
 read_edge=$realtime;
end
"""
    assert 'LabPinModel #(' in tb
    tb=tb.replace('LabPinModel #(', pads+'LabPinModel #(',1)
    for pin,wire in [('cke','O_sdram_cke'),('cs','O_sdram_cs_n'),('ras','O_sdram_ras_n'),('cas','O_sdram_cas_n'),('we','O_sdram_wen_n'),('dqm','O_sdram_dqm'),('a','O_sdram_addr'),('ba','O_sdram_ba'),('dq','IO_sdram_dq')]:
        destination={'cke':'pin_cke','cs':'pin_cs','ras':'pin_ras','cas':'pin_cas','we':'pin_we','dqm':'pin_dqm','a':'pin_a','ba':'pin_ba','dq':'pin_dq'}[pin]
        tb=tb.replace(f'.{pin}({wire})',f'.{pin}({destination})')
    return tb
