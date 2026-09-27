DPB #(.READ_MODE0(1'b0),.READ_MODE1(1'b0),.WRITE_MODE0(2'b00),.WRITE_MODE1(2'b00),
 .BIT_WIDTH_0(16),.BIT_WIDTH_1(16),.BLK_SEL_0(3'b0),.BLK_SEL_1(3'b0),.RESET_MODE("SYNC")) bank_0(
 .DOA(bank_0_a_read_data),.DOB(bank_0_b_read_data),.DIA(bank_0_a_write_data),.DIB(bank_0_b_write_data),
 .ADA({bank_0_a_address,2'b0,2'b11}),.ADB({bank_0_b_address,2'b0,2'b11}),
 .BLKSELA(3'b0),.BLKSELB(3'b0),.WREA(bank_0_a_write_enable),.WREB(bank_0_b_write_enable),
 .CLKA(clk),.CLKB(clk),.CEA(1'b1),.CEB(1'b1),.OCEA(1'b0),.OCEB(1'b0),.RESETA(1'b0),.RESETB(1'b0));
DPB #(.READ_MODE0(1'b0),.READ_MODE1(1'b0),.WRITE_MODE0(2'b00),.WRITE_MODE1(2'b00),
 .BIT_WIDTH_0(16),.BIT_WIDTH_1(16),.BLK_SEL_0(3'b0),.BLK_SEL_1(3'b0),.RESET_MODE("SYNC")) bank_1(
 .DOA(bank_1_a_read_data),.DOB(bank_1_b_read_data),.DIA(bank_1_a_write_data),.DIB(bank_1_b_write_data),
 .ADA({bank_1_a_address,2'b0,2'b11}),.ADB({bank_1_b_address,2'b0,2'b11}),
 .BLKSELA(3'b0),.BLKSELB(3'b0),.WREA(bank_1_a_write_enable),.WREB(bank_1_b_write_enable),
 .CLKA(clk),.CLKB(clk),.CEA(1'b1),.CEB(1'b1),.OCEA(1'b0),.OCEB(1'b0),.RESETA(1'b0),.RESETB(1'b0));
endmodule
