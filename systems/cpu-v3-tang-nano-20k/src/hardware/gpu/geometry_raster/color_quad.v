// Private color-only output queue. No framebuffer/cache/merger ports.
// Work: x[8:0],y[16:9],source[23:17],primitive[26:24],kind[28:27],
// last-source[29],reserved[30]. Kind: quad=0,tile-end=1,source-end=2,context-end=3.
module GpuColorQuad(input wire clk,input wire reset,
    input wire input_valid,output wire input_ready,input wire [30:0] input_work,
    input wire [3:0] input_mask,input wire [95:0] input_denominator,
    input wire [359:0] input_numerator,
    output wire output_valid,input wire output_ready,output wire [30:0] output_work,
    output wire [3:0] output_mask,output wire [63:0] output_color,output wire output_error,output wire idle);
reg holding;reg [30:0] work;reg [1:0] mask;reg [47:0] denominator;reg [179:0] numerator;
wire pair_ready,queue_idle;
assign idle=!holding && queue_idle;
assign input_ready=!holding && pair_ready;
GpuColorPairQueue queue(clk,reset,holding || input_valid,pair_ready,
    {holding,holding ? work : input_work},holding ? mask : input_mask[1:0],
    holding ? denominator : input_denominator[47:0],holding ? numerator : input_numerator[179:0],
    output_valid,output_ready,output_work,output_mask,output_color,output_error,queue_idle);
always @(posedge clk) begin
    if(reset) begin holding<=0;work<=0;mask<=0;denominator<=0;numerator<=0;end
    else if(!holding && input_valid && pair_ready) begin
        holding<=1;work<=input_work;mask<=input_mask[3:2];denominator<=input_denominator[95:48];numerator<=input_numerator[359:180];
    end else if(holding && pair_ready) holding<=0;
end
endmodule
module GpuColorPairQueue(input wire clk,input wire reset,
    input wire input_valid,output wire input_ready,input wire [31:0] input_tag,
    input wire [1:0] input_mask,input wire [47:0] input_denominator,input wire [179:0] input_numerator,
    output wire output_valid,input wire output_ready,output wire [30:0] output_work,
    output wire [3:0] output_mask,output wire [63:0] output_color,output wire output_error,output wire idle);
wire pair_valid;wire [31:0] pair_tag,pair_color;wire [1:0] pair_mask;wire pair_error,shade_ready,shader_idle;
reg first_pending;

GpuColorVarying shader(clk,reset,input_valid,input_ready,input_tag,input_mask,input_denominator,input_numerator,
    pair_valid,shade_ready,pair_tag,pair_mask,pair_color,pair_error,shader_idle);
reg [31:0] first_color;
reg [1:0] first_mask;
reg first_error;
reg [30:0] first_work;
wire queue_ready;
assign shade_ready=!pair_tag[31] || queue_ready;
wire push=pair_valid && pair_tag[31] && queue_ready;
wire [107:0] write_data={8'd0,(first_error|pair_error),pair_color,first_color,pair_mask,first_mask,pair_tag[30:0]};
wire [107:0] read_data;
reg [3:0] head,tail;
reg [4:0] count;
assign idle=count==0 && shader_idle && !first_pending;
reg head_ready;
wire pop=output_valid && output_ready;
assign queue_ready=count<16 || pop;
assign output_valid=count!=0 && head_ready;
assign output_work=read_data[30:0];assign output_mask=read_data[34:31];
assign output_color=read_data[98:35];assign output_error=read_data[99];
`ifdef __ICARUS__
    reg [107:0] memory[0:15];reg [107:0] read_register;
    assign read_data=read_register;
    always @(posedge clk) begin
        read_register<=memory[head];if(push) memory[tail]<=write_data;
    end
`else
    genvar bank;
    generate for(bank=0;bank<3;bank=bank+1) begin: queue_bank
        SDPX9B #(.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.READ_MODE(1'b0)) ram(
            .CLKA(clk),.CLKB(clk),.CEA(push),.CEB(1'b1),.OCE(1'b0),
            .RESETA(1'b0),.RESETB(1'b0),.BLKSELA(3'd0),.BLKSELB(3'd0),
            .ADA({5'd0,tail,1'b0,4'b1111}),.ADB({5'd0,head,5'd0}),
            .DI(write_data[bank*36+:36]),.DO(read_data[bank*36+:36]));
    end endgenerate
`endif
always @(posedge clk) begin
    if(reset) begin
        first_pending<=0;first_color<=0;first_mask<=0;first_work<=0;first_error<=0;
        head<=0;tail<=0;count<=0;head_ready<=0;
    end else begin
        if(pair_valid && shade_ready && !pair_tag[31]) begin
            first_pending<=1;first_color<=pair_color;first_mask<=pair_mask;first_work<=pair_tag[30:0];first_error<=pair_error;
        end
        if(push) begin tail<=tail+1'b1;first_pending<=0;end
        if(pop) begin head<=head+1'b1;head_ready<=0;end
        else if(count!=0) head_ready<=1;
        case({push,pop}) 2'b10:count<=count+1'b1;2'b01:count<=count-1'b1;endcase
        // synthesis translate_off
        if(push && pair_tag[30:0]!=first_work) $fatal(1,"color quad pair identity mismatch");
        if(count>16) $fatal(1,"color quad queue overflow");
        // synthesis translate_on
    end
end
endmodule
