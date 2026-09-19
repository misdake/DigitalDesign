// Testbench for CpuV3FpuSpecialPath.
//
// The register file is a behavioral stub with the two asymmetric mirrors
// (mirror 0 = RCP at 64..191, mirror 1 = RSQRT even at 64..191 and odd at
// 192..319) initialized with the frozen section-9.4 tables. The path is driven
// directly with
// instr_complete / instr_opcode / word1_raw, exactly like the other
// execution-path TBs.
//
// Operand delivery mirrors the parent: the front-end parks the operand address
// on RF port A one beat before T0, so the synchronous RF presents the operand
// on rf_read_a_data during T0. From T0 on the path drives its own LUT address;
// the RF presents the LUT line one beat later (T1). This TB therefore keeps
// the operand in architectural register F0 and drives the parked address 0
// until T0.
//
// The reference model is an independent behavioral Verilog re-implementation
// of the RCP/RSQRT algorithms of fpu-design-v2 section 9.4: normalize, LUT
// interpolation with the endpoint substitution, and the power-of-two rescale,
// written without reusing any leaf logic. It is checked against the RTL for
// the directed corners and a deterministic randomized sweep of >= 2000 inputs
// per function.
module tb;
reg clk = 0;
always #5 clk = ~clk;

localparam integer MAX_CYCLES = 4000000;
integer cycles = 0;
always @(posedge clk) begin
    cycles <= cycles + 1;
    if (cycles > MAX_CYCLES) begin
        $display("DIGITAL_DESIGN_FAIL: cycle limit exceeded");
        $finish;
    end
end

localparam [5:0] RCP_SUBOP = 6'h0C;
localparam [5:0] RSQRT_SUBOP = 6'h0D;

// Instruction inputs.
reg instr_complete = 0;
reg [3:0] instr_opcode = 0;
reg [15:0] word1_raw = 0;
reg abort = 0;

// Register-file write port: TB preparation writes vs special-path writes.
reg prep_write_enable = 0;
reg [8:0] prep_write_address = 0;
reg [31:0] prep_write_data = 0;

wire [8:0] sf_read_a_address;
wire [8:0] sf_read_b_address;
wire sf_write_enable;
wire [8:0] sf_write_address;
wire [31:0] sf_write_data;
wire busy;
wire [3:0] sf_r_wait;
wire [3:0] sf_w_wait;
wire [3:0] sf_x_wait;

// Read address seen by the RF: the parked operand address before T0, the
// path's LUT address from T0 on (the path asserts busy for T0..T3). Readback
// for TB inspection uses the same port while idle.
reg [8:0] parked_a = 9'd0;
reg rb_sel = 0;
reg [8:0] rb_a = 0;
reg [8:0] rb_b = 0;
wire [8:0] eff_read_a = rb_sel ? rb_a : (busy ? sf_read_a_address : parked_a);
wire [8:0] eff_read_b = rb_sel ? rb_b : sf_read_b_address;

wire [31:0] rf_read_a_data;
wire [31:0] rf_read_b_data;

// Behavioral 2R1W register file with the two asymmetric LUT mirrors.
reg [31:0] mirror_0 [0:511];
reg [31:0] mirror_1 [0:511];
reg [31:0] rf_read_a_data_r = 32'b0;
reg [31:0] rf_read_b_data_r = 32'b0;
always @(posedge clk) begin
    if (prep_write_enable) begin
        mirror_0[prep_write_address] <= prep_write_data;
        mirror_1[prep_write_address] <= prep_write_data;
    end
    if (sf_write_enable)
        mirror_0[sf_write_address] <= sf_write_data;
    rf_read_a_data_r <= mirror_0[eff_read_a];
    rf_read_b_data_r <= mirror_1[eff_read_b];
end
assign rf_read_a_data = rf_read_a_data_r;
assign rf_read_b_data = rf_read_b_data_r;

// LUT tables, the frozen section-9.4 values.
integer t;
initial begin
    for (t = 0; t < 512; t = t + 1) begin
        mirror_0[t] = 32'b0;
        mirror_1[t] = 32'b0;
    end
    // RCP (mirror 0, 64..191).
    mirror_0[64] = 32'h00010000; mirror_0[65] = 32'h0000FE04; mirror_0[66] = 32'h0000FC10; mirror_0[67] = 32'h0000FA23;
    mirror_0[68] = 32'h0000F83E; mirror_0[69] = 32'h0000F660; mirror_0[70] = 32'h0000F48A; mirror_0[71] = 32'h0000F2BA;
    mirror_0[72] = 32'h0000F0F1; mirror_0[73] = 32'h0000EF2F; mirror_0[74] = 32'h0000ED73; mirror_0[75] = 32'h0000EBBE;
    mirror_0[76] = 32'h0000EA0F; mirror_0[77] = 32'h0000E866; mirror_0[78] = 32'h0000E6C3; mirror_0[79] = 32'h0000E526;
    mirror_0[80] = 32'h0000E38E; mirror_0[81] = 32'h0000E1FC; mirror_0[82] = 32'h0000E070; mirror_0[83] = 32'h0000DEE9;
    mirror_0[84] = 32'h0000DD68; mirror_0[85] = 32'h0000DBEB; mirror_0[86] = 32'h0000DA74; mirror_0[87] = 32'h0000D902;
    mirror_0[88] = 32'h0000D794; mirror_0[89] = 32'h0000D62C; mirror_0[90] = 32'h0000D4C7; mirror_0[91] = 32'h0000D368;
    mirror_0[92] = 32'h0000D20D; mirror_0[93] = 32'h0000D0B7; mirror_0[94] = 32'h0000CF64; mirror_0[95] = 32'h0000CE17;
    mirror_0[96] = 32'h0000CCCD; mirror_0[97] = 32'h0000CB87; mirror_0[98] = 32'h0000CA46; mirror_0[99] = 32'h0000C908;
    mirror_0[100] = 32'h0000C7CE; mirror_0[101] = 32'h0000C698; mirror_0[102] = 32'h0000C566; mirror_0[103] = 32'h0000C437;
    mirror_0[104] = 32'h0000C30C; mirror_0[105] = 32'h0000C1E5; mirror_0[106] = 32'h0000C0C1; mirror_0[107] = 32'h0000BFA0;
    mirror_0[108] = 32'h0000BE83; mirror_0[109] = 32'h0000BD69; mirror_0[110] = 32'h0000BC52; mirror_0[111] = 32'h0000BB3F;
    mirror_0[112] = 32'h0000BA2F; mirror_0[113] = 32'h0000B921; mirror_0[114] = 32'h0000B817; mirror_0[115] = 32'h0000B710;
    mirror_0[116] = 32'h0000B60B; mirror_0[117] = 32'h0000B50A; mirror_0[118] = 32'h0000B40B; mirror_0[119] = 32'h0000B30F;
    mirror_0[120] = 32'h0000B216; mirror_0[121] = 32'h0000B120; mirror_0[122] = 32'h0000B02C; mirror_0[123] = 32'h0000AF3B;
    mirror_0[124] = 32'h0000AE4C; mirror_0[125] = 32'h0000AD60; mirror_0[126] = 32'h0000AC77; mirror_0[127] = 32'h0000AB8F;
    mirror_0[128] = 32'h0000AAAB; mirror_0[129] = 32'h0000A9C8; mirror_0[130] = 32'h0000A8E8; mirror_0[131] = 32'h0000A80B;
    mirror_0[132] = 32'h0000A72F; mirror_0[133] = 32'h0000A656; mirror_0[134] = 32'h0000A57F; mirror_0[135] = 32'h0000A4AA;
    mirror_0[136] = 32'h0000A3D7; mirror_0[137] = 32'h0000A306; mirror_0[138] = 32'h0000A238; mirror_0[139] = 32'h0000A16B;
    mirror_0[140] = 32'h0000A0A1; mirror_0[141] = 32'h00009FD8; mirror_0[142] = 32'h00009F11; mirror_0[143] = 32'h00009E4D;
    mirror_0[144] = 32'h00009D8A; mirror_0[145] = 32'h00009CC9; mirror_0[146] = 32'h00009C0A; mirror_0[147] = 32'h00009B4C;
    mirror_0[148] = 32'h00009A91; mirror_0[149] = 32'h000099D7; mirror_0[150] = 32'h0000991F; mirror_0[151] = 32'h00009869;
    mirror_0[152] = 32'h000097B4; mirror_0[153] = 32'h00009701; mirror_0[154] = 32'h00009650; mirror_0[155] = 32'h000095A0;
    mirror_0[156] = 32'h000094F2; mirror_0[157] = 32'h00009446; mirror_0[158] = 32'h0000939B; mirror_0[159] = 32'h000092F1;
    mirror_0[160] = 32'h00009249; mirror_0[161] = 32'h000091A3; mirror_0[162] = 32'h000090FE; mirror_0[163] = 32'h0000905A;
    mirror_0[164] = 32'h00008FB8; mirror_0[165] = 32'h00008F17; mirror_0[166] = 32'h00008E78; mirror_0[167] = 32'h00008DDA;
    mirror_0[168] = 32'h00008D3E; mirror_0[169] = 32'h00008CA3; mirror_0[170] = 32'h00008C09; mirror_0[171] = 32'h00008B70;
    mirror_0[172] = 32'h00008AD9; mirror_0[173] = 32'h00008A43; mirror_0[174] = 32'h000089AE; mirror_0[175] = 32'h0000891B;
    mirror_0[176] = 32'h00008889; mirror_0[177] = 32'h000087F8; mirror_0[178] = 32'h00008768; mirror_0[179] = 32'h000086D9;
    mirror_0[180] = 32'h0000864C; mirror_0[181] = 32'h000085BF; mirror_0[182] = 32'h00008534; mirror_0[183] = 32'h000084AA;
    mirror_0[184] = 32'h00008421; mirror_0[185] = 32'h00008399; mirror_0[186] = 32'h00008312; mirror_0[187] = 32'h0000828D;
    mirror_0[188] = 32'h00008208; mirror_0[189] = 32'h00008185; mirror_0[190] = 32'h00008102; mirror_0[191] = 32'h00008081;
    // RSQRT even, 1/sqrt(m) (mirror 1, 64..191).
    mirror_1[64] = 32'h00010000; mirror_1[65] = 32'h0000FF01; mirror_1[66] = 32'h0000FE06; mirror_1[67] = 32'h0000FD0D;
    mirror_1[68] = 32'h0000FC17; mirror_1[69] = 32'h0000FB24; mirror_1[70] = 32'h0000FA34; mirror_1[71] = 32'h0000F946;
    mirror_1[72] = 32'h0000F85B; mirror_1[73] = 32'h0000F773; mirror_1[74] = 32'h0000F68D; mirror_1[75] = 32'h0000F5A9;
    mirror_1[76] = 32'h0000F4C8; mirror_1[77] = 32'h0000F3EA; mirror_1[78] = 32'h0000F30E; mirror_1[79] = 32'h0000F234;
    mirror_1[80] = 32'h0000F15C; mirror_1[81] = 32'h0000F087; mirror_1[82] = 32'h0000EFB3; mirror_1[83] = 32'h0000EEE2;
    mirror_1[84] = 32'h0000EE13; mirror_1[85] = 32'h0000ED46; mirror_1[86] = 32'h0000EC7C; mirror_1[87] = 32'h0000EBB3;
    mirror_1[88] = 32'h0000EAEC; mirror_1[89] = 32'h0000EA27; mirror_1[90] = 32'h0000E964; mirror_1[91] = 32'h0000E8A3;
    mirror_1[92] = 32'h0000E7E4; mirror_1[93] = 32'h0000E727; mirror_1[94] = 32'h0000E66B; mirror_1[95] = 32'h0000E5B1;
    mirror_1[96] = 32'h0000E4F9; mirror_1[97] = 32'h0000E443; mirror_1[98] = 32'h0000E38E; mirror_1[99] = 32'h0000E2DB;
    mirror_1[100] = 32'h0000E22A; mirror_1[101] = 32'h0000E17A; mirror_1[102] = 32'h0000E0CC; mirror_1[103] = 32'h0000E020;
    mirror_1[104] = 32'h0000DF75; mirror_1[105] = 32'h0000DECB; mirror_1[106] = 32'h0000DE23; mirror_1[107] = 32'h0000DD7C;
    mirror_1[108] = 32'h0000DCD7; mirror_1[109] = 32'h0000DC34; mirror_1[110] = 32'h0000DB92; mirror_1[111] = 32'h0000DAF1;
    mirror_1[112] = 32'h0000DA51; mirror_1[113] = 32'h0000D9B3; mirror_1[114] = 32'h0000D916; mirror_1[115] = 32'h0000D87B;
    mirror_1[116] = 32'h0000D7E1; mirror_1[117] = 32'h0000D748; mirror_1[118] = 32'h0000D6B0; mirror_1[119] = 32'h0000D61A;
    mirror_1[120] = 32'h0000D585; mirror_1[121] = 32'h0000D4F1; mirror_1[122] = 32'h0000D45E; mirror_1[123] = 32'h0000D3CD;
    mirror_1[124] = 32'h0000D33C; mirror_1[125] = 32'h0000D2AD; mirror_1[126] = 32'h0000D21F; mirror_1[127] = 32'h0000D192;
    mirror_1[128] = 32'h0000D106; mirror_1[129] = 32'h0000D07B; mirror_1[130] = 32'h0000CFF1; mirror_1[131] = 32'h0000CF69;
    mirror_1[132] = 32'h0000CEE1; mirror_1[133] = 32'h0000CE5A; mirror_1[134] = 32'h0000CDD5; mirror_1[135] = 32'h0000CD50;
    mirror_1[136] = 32'h0000CCCD; mirror_1[137] = 32'h0000CC4A; mirror_1[138] = 32'h0000CBC9; mirror_1[139] = 32'h0000CB48;
    mirror_1[140] = 32'h0000CAC8; mirror_1[141] = 32'h0000CA49; mirror_1[142] = 32'h0000C9CC; mirror_1[143] = 32'h0000C94F;
    mirror_1[144] = 32'h0000C8D3; mirror_1[145] = 32'h0000C858; mirror_1[146] = 32'h0000C7DD; mirror_1[147] = 32'h0000C764;
    mirror_1[148] = 32'h0000C6EB; mirror_1[149] = 32'h0000C674; mirror_1[150] = 32'h0000C5FD; mirror_1[151] = 32'h0000C587;
    mirror_1[152] = 32'h0000C512; mirror_1[153] = 32'h0000C49D; mirror_1[154] = 32'h0000C42A; mirror_1[155] = 32'h0000C3B7;
    mirror_1[156] = 32'h0000C345; mirror_1[157] = 32'h0000C2D4; mirror_1[158] = 32'h0000C263; mirror_1[159] = 32'h0000C1F4;
    mirror_1[160] = 32'h0000C185; mirror_1[161] = 32'h0000C116; mirror_1[162] = 32'h0000C0A9; mirror_1[163] = 32'h0000C03C;
    mirror_1[164] = 32'h0000BFD0; mirror_1[165] = 32'h0000BF65; mirror_1[166] = 32'h0000BEFA; mirror_1[167] = 32'h0000BE90;
    mirror_1[168] = 32'h0000BE27; mirror_1[169] = 32'h0000BDBE; mirror_1[170] = 32'h0000BD56; mirror_1[171] = 32'h0000BCEF;
    mirror_1[172] = 32'h0000BC89; mirror_1[173] = 32'h0000BC23; mirror_1[174] = 32'h0000BBBD; mirror_1[175] = 32'h0000BB59;
    mirror_1[176] = 32'h0000BAF5; mirror_1[177] = 32'h0000BA91; mirror_1[178] = 32'h0000BA2F; mirror_1[179] = 32'h0000B9CC;
    mirror_1[180] = 32'h0000B96B; mirror_1[181] = 32'h0000B90A; mirror_1[182] = 32'h0000B8A9; mirror_1[183] = 32'h0000B84A;
    mirror_1[184] = 32'h0000B7EA; mirror_1[185] = 32'h0000B78C; mirror_1[186] = 32'h0000B72E; mirror_1[187] = 32'h0000B6D0;
    mirror_1[188] = 32'h0000B673; mirror_1[189] = 32'h0000B617; mirror_1[190] = 32'h0000B5BB; mirror_1[191] = 32'h0000B560;
    // RSQRT odd, 1/sqrt(2m) (mirror 1, 192..319).
    mirror_1[192] = 32'h0000B505; mirror_1[193] = 32'h0000B451; mirror_1[194] = 32'h0000B39F; mirror_1[195] = 32'h0000B2EF;
    mirror_1[196] = 32'h0000B241; mirror_1[197] = 32'h0000B196; mirror_1[198] = 32'h0000B0EC; mirror_1[199] = 32'h0000B044;
    mirror_1[200] = 32'h0000AF9D; mirror_1[201] = 32'h0000AEF9; mirror_1[202] = 32'h0000AE56; mirror_1[203] = 32'h0000ADB6;
    mirror_1[204] = 32'h0000AD16; mirror_1[205] = 32'h0000AC79; mirror_1[206] = 32'h0000ABDD; mirror_1[207] = 32'h0000AB43;
    mirror_1[208] = 32'h0000AAAB; mirror_1[209] = 32'h0000AA14; mirror_1[210] = 32'h0000A97E; mirror_1[211] = 32'h0000A8EB;
    mirror_1[212] = 32'h0000A858; mirror_1[213] = 32'h0000A7C7; mirror_1[214] = 32'h0000A738; mirror_1[215] = 32'h0000A6AA;
    mirror_1[216] = 32'h0000A61D; mirror_1[217] = 32'h0000A592; mirror_1[218] = 32'h0000A508; mirror_1[219] = 32'h0000A480;
    mirror_1[220] = 32'h0000A3F9; mirror_1[221] = 32'h0000A373; mirror_1[222] = 32'h0000A2EE; mirror_1[223] = 32'h0000A26B;
    mirror_1[224] = 32'h0000A1E9; mirror_1[225] = 32'h0000A168; mirror_1[226] = 32'h0000A0E8; mirror_1[227] = 32'h0000A069;
    mirror_1[228] = 32'h00009FEC; mirror_1[229] = 32'h00009F70; mirror_1[230] = 32'h00009EF5; mirror_1[231] = 32'h00009E7B;
    mirror_1[232] = 32'h00009E02; mirror_1[233] = 32'h00009D8A; mirror_1[234] = 32'h00009D13; mirror_1[235] = 32'h00009C9D;
    mirror_1[236] = 32'h00009C29; mirror_1[237] = 32'h00009BB5; mirror_1[238] = 32'h00009B42; mirror_1[239] = 32'h00009AD0;
    mirror_1[240] = 32'h00009A60; mirror_1[241] = 32'h000099F0; mirror_1[242] = 32'h00009981; mirror_1[243] = 32'h00009913;
    mirror_1[244] = 32'h000098A6; mirror_1[245] = 32'h0000983A; mirror_1[246] = 32'h000097CF; mirror_1[247] = 32'h00009764;
    mirror_1[248] = 32'h000096FB; mirror_1[249] = 32'h00009692; mirror_1[250] = 32'h0000962B; mirror_1[251] = 32'h000095C4;
    mirror_1[252] = 32'h0000955E; mirror_1[253] = 32'h000094F8; mirror_1[254] = 32'h00009494; mirror_1[255] = 32'h00009430;
    mirror_1[256] = 32'h000093CD; mirror_1[257] = 32'h0000936B; mirror_1[258] = 32'h0000930A; mirror_1[259] = 32'h000092A9;
    mirror_1[260] = 32'h00009249; mirror_1[261] = 32'h000091EA; mirror_1[262] = 32'h0000918C; mirror_1[263] = 32'h0000912E;
    mirror_1[264] = 32'h000090D1; mirror_1[265] = 32'h00009074; mirror_1[266] = 32'h00009019; mirror_1[267] = 32'h00008FBE;
    mirror_1[268] = 32'h00008F64; mirror_1[269] = 32'h00008F0A; mirror_1[270] = 32'h00008EB1; mirror_1[271] = 32'h00008E59;
    mirror_1[272] = 32'h00008E01; mirror_1[273] = 32'h00008DAA; mirror_1[274] = 32'h00008D53; mirror_1[275] = 32'h00008CFD;
    mirror_1[276] = 32'h00008CA8; mirror_1[277] = 32'h00008C54; mirror_1[278] = 32'h00008C00; mirror_1[279] = 32'h00008BAC;
    mirror_1[280] = 32'h00008B59; mirror_1[281] = 32'h00008B07; mirror_1[282] = 32'h00008AB5; mirror_1[283] = 32'h00008A64;
    mirror_1[284] = 32'h00008A13; mirror_1[285] = 32'h000089C3; mirror_1[286] = 32'h00008974; mirror_1[287] = 32'h00008925;
    mirror_1[288] = 32'h000088D6; mirror_1[289] = 32'h00008889; mirror_1[290] = 32'h0000883B; mirror_1[291] = 32'h000087EE;
    mirror_1[292] = 32'h000087A2; mirror_1[293] = 32'h00008756; mirror_1[294] = 32'h0000870B; mirror_1[295] = 32'h000086C0;
    mirror_1[296] = 32'h00008675; mirror_1[297] = 32'h0000862B; mirror_1[298] = 32'h000085E2; mirror_1[299] = 32'h00008599;
    mirror_1[300] = 32'h00008550; mirror_1[301] = 32'h00008508; mirror_1[302] = 32'h000084C1; mirror_1[303] = 32'h00008479;
    mirror_1[304] = 32'h00008433; mirror_1[305] = 32'h000083EC; mirror_1[306] = 32'h000083A7; mirror_1[307] = 32'h00008361;
    mirror_1[308] = 32'h0000831C; mirror_1[309] = 32'h000082D8; mirror_1[310] = 32'h00008293; mirror_1[311] = 32'h00008250;
    mirror_1[312] = 32'h0000820C; mirror_1[313] = 32'h000081C9; mirror_1[314] = 32'h00008187; mirror_1[315] = 32'h00008145;
    mirror_1[316] = 32'h00008103; mirror_1[317] = 32'h000080C2; mirror_1[318] = 32'h00008081; mirror_1[319] = 32'h00008040;
end

CpuV3FpuSpecialPath special_path (
    .clk(clk),
    .abort(abort),
    .instr_complete(instr_complete),
    .instr_opcode(instr_opcode),
    .word1_raw(word1_raw),
    .rf_read_a_data(rf_read_a_data),
    .rf_read_b_data(rf_read_b_data),
    .rf_read_a_address(sf_read_a_address),
    .rf_read_b_address(sf_read_b_address),
    .rf_write_enable(sf_write_enable),
    .rf_write_address(sf_write_address),
    .rf_write_data(sf_write_data),
    .busy(busy),
    .r_wait(sf_r_wait),
    .w_wait(sf_w_wait),
    .x_wait(sf_x_wait)
);

integer check_count = 0;
integer seed = 32'h5BEC1A17;

task fail;
    input [8*96-1:0] msg;
    begin
        $display("DIGITAL_DESIGN_FAIL: %0s", msg);
        $finish;
    end
endtask

task check_value;
    input [31:0] got;
    input [31:0] want;
    input [8*64-1:0] label;
    begin
        check_count = check_count + 1;
        if (got !== want) begin
            $display("DIGITAL_DESIGN_FAIL: %0s got %08h want %08h",
                label, got, want);
            $finish;
        end
    end
endtask

// ---------------------------------------------------------------------------
// Reference model: independent behavioral RCP/RSQRT.
// ---------------------------------------------------------------------------
function integer clz_ref;
    input [31:0] value;
    integer k;
    begin
        clz_ref = 32;
        for (k = 31; k >= 0; k = k - 1) begin
            if (value[k] == 1'b1 && clz_ref == 32)
                clz_ref = 31 - k;
        end
    end
endfunction

function [31:0] interp_ref;
    input [31:0] lut_current;
    input [31:0] lut_next;
    input integer residue;
    input integer shift;
    reg signed [63:0] delta;
    begin
        delta = $signed(lut_next) - $signed(lut_current);
        interp_ref = lut_current + ((delta * residue) >>> shift);
    end
endfunction

function [31:0] rcp_ref;
    input [31:0] x;
    integer clz;
    integer index;
    integer residue;
    integer shift;
    reg [31:0] absx;
    reg [63:0] norm;
    reg [31:0] interp;
    reg [31:0] lut_next;
    reg [63:0] wide;
    begin
        if (x == 32'd0) begin
            rcp_ref = 32'h7FFFFFFF;
        end else begin
            absx = x[31] ? (32'd0 - x) : x;
            clz = clz_ref(absx);
            norm = {32'b0, absx} << clz;
            index = (norm >> 24) & 7'h7F;
            residue = (norm >> 15) & 9'h1FF;
            if (index == 127)
                lut_next = 32'h00008000;
            else
                lut_next = mirror_0[64 + index + 1];
            interp = interp_ref(mirror_0[64 + index], lut_next, residue, 9);
            shift = clz - 15;
            if (shift >= 0) begin
                wide = {32'b0, interp} << shift;
                if (wide > 64'h7FFFFFFF)
                    rcp_ref = 32'h7FFFFFFF;
                else
                    rcp_ref = wide[31:0];
            end else begin
                rcp_ref = interp >> (-shift);
            end
            if (x[31])
                rcp_ref = 32'd0 - rcp_ref;
        end
    end
endfunction

function [31:0] rsqrt_ref;
    input [31:0] x;
    integer clz;
    integer be;
    integer odd;
    integer base;
    integer index;
    integer residue;
    integer scale_shift;
    reg [31:0] norm;
    reg [31:0] interp;
    reg [31:0] lut_next;
    begin
        if (x[31] == 1'b1 || x == 32'd0) begin
            rsqrt_ref = 32'd0;
        end else begin
            clz = clz_ref(x);
            be = 31 - clz;
            odd = be & 1;
            norm = x << clz;
            index = (norm >> 24) & 7'h7F;
            residue = (norm >> 15) & 9'h1FF;
            base = odd ? 192 : 64;
            if (index == 127)
                lut_next = odd ? 32'h00008000 : 32'h0000B505;
            else
                lut_next = mirror_1[base + index + 1];
            interp = interp_ref(mirror_1[base + index], lut_next, residue, 9);
            scale_shift = (be - 16 - odd) / 2;
            if (scale_shift >= 0)
                rsqrt_ref = interp >> scale_shift;
            else
                rsqrt_ref = interp << (-scale_shift);
        end
    end
endfunction

// Prepares F0 with the operand through the RF write port (parked address 0).
task set_operand;
    input [31:0] x;
    begin
        @(negedge clk);
        prep_write_enable = 1'b1;
        prep_write_address = 9'd0;
        prep_write_data = x;
        @(posedge clk);
        #1;
        prep_write_enable = 1'b0;
        @(negedge clk);
    end
endtask

// Issues one special instruction and checks the T0..T3 window and the written
// word against `expected`.
task run_special;
    input [5:0] fd;
    input [5:0] subop;
    input [31:0] x;
    input [31:0] expected;
    input [8*64-1:0] label;
    integer t;
    integer busy_count;
    begin
        set_operand(x);

        // Operand address parked on port A; the RF latches it at this edge so
        // it is on rf_read_a_data during T0.
        parked_a = 9'd0;
        @(negedge clk);
        #1;
        instr_complete = 1'b1;
        instr_opcode = 4'hD;
        word1_raw = {fd, subop, 4'h0};
        #1;
        check_value({31'b0, busy}, 32'h1, "T0 busy");
        check_value({31'b0, sf_w_wait}, 32'h4, "T0 w_wait");
        check_value({31'b0, sf_x_wait}, 32'h4, "T0 x_wait");
        check_value({31'b0, sf_r_wait}, 32'h0, "T0 r_wait");
        check_value({31'b0, sf_write_enable}, 32'h0, "T0 write idle");

        busy_count = 1;
        @(negedge clk);
        instr_complete = 1'b0;
        #1;
        t = 1;
        while (busy) begin
            if (t == 3) begin
                check_value({31'b0, sf_write_enable}, 32'h1, "T3 write enable");
                check_value({23'b0, sf_write_address}, fd, "T3 write address");
                check_value(sf_write_data, expected, label);
            end else begin
                check_value({31'b0, sf_write_enable}, 32'h0, "write idle");
            end
            busy_count = busy_count + 1;
            t = t + 1;
            @(negedge clk);
            #1;
        end
        check_value(busy_count, 4, "busy window length");
        check_value({31'b0, busy}, 32'h0, "busy clear");
        check_value({31'b0, sf_write_enable}, 32'h0, "write clear");
        @(negedge clk);
    end
endtask

task run_rcp;
    input [5:0] fd;
    input [31:0] x;
    begin
        run_special(fd, RCP_SUBOP, x, rcp_ref(x), "rcp ref");
    end
endtask

task run_rsqrt;
    input [5:0] fd;
    input [31:0] x;
    begin
        run_special(fd, RSQRT_SUBOP, x, rsqrt_ref(x), "rsqrt ref");
    end
endtask

integer i;
reg [31:0] rnd;
reg [31:0] rnd_x;

initial begin
    run_rcp(6'd0, 32'h00010000); //  1.0
    run_rcp(6'd1, 32'h00040000); //  4.0
    run_rcp(6'd2, 32'h00008000); //  0.5
    run_rcp(6'd3, 32'h00000000); //  0
    run_rcp(6'd4, 32'hFFFF0000); // -1.0
    run_rcp(6'd5, 32'h7FFFFFFF); //  max
    run_rcp(6'd6, 32'h80000000); //  min
    run_rcp(6'd7, 32'h00000001); //  tiny
    run_rcp(6'd8, 32'hFFFFFFFF); // -1 LSB
    run_rcp(6'd9, 32'h00020000); //  2.0

    run_rsqrt(6'd0, 32'h00040000); //  4.0
    run_rsqrt(6'd1, 32'h00004000); //  0.25
    run_rsqrt(6'd2, 32'h00010000); //  1.0
    run_rsqrt(6'd3, 32'h00000000); //  0
    run_rsqrt(6'd4, 32'hFFFF0000); // -1.0
    run_rsqrt(6'd5, 32'h00000001); //  tiny
    run_rsqrt(6'd6, 32'h7FFFFFFF); //  max
    run_rsqrt(6'd7, 32'h00090000); //  9.0
    run_rsqrt(6'd8, 32'h00080000); //  8.0
    run_rsqrt(6'd9, 32'h00020000); //  2.0

    for (i = 0; i < 2500; i = i + 1) begin
        rnd = $random(seed);
        run_rcp(6'd10, rnd);
    end

    for (i = 0; i < 2500; i = i + 1) begin
        rnd = $random(seed);
        rnd_x = rnd & 32'h7FFFFFFF;
        if (rnd_x == 32'd0)
            rnd_x = 32'd1;
        run_rsqrt(6'd11, rnd_x);
    end
    for (i = 0; i < 500; i = i + 1) begin
        run_rsqrt(6'd12, (i * 65537) + 1);
    end

    if (check_count == 0)
        fail("no checks executed");
    $display("checks=%0d", check_count);
    $display("cycles=%0d", cycles);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
