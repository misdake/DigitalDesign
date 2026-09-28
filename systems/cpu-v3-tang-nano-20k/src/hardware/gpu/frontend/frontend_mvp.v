// One 36x36 multiplier, fully registered in the DSP. Sixteen consecutive
// issues transform XYZW; no implicit W and no intermediate row rounding.
// Coefficients stream from scratchpad instead of a 512-FF matrix register.
module FrontendMultiply36(input wire clk, input wire reset,
    input wire signed [35:0] a, input wire signed [35:0] b,
    output wire signed [71:0] product);
`ifdef __ICARUS__
    reg signed [35:0] ar,br;
    reg signed [71:0] pipe, result;
    assign product=result;
    always @(posedge clk) begin
        if (reset) begin ar<=0; br<=0; pipe<=0; result<=0; end
        else begin ar<=a; br<=b; pipe<=ar*br; result<=pipe; end
    end
`else
    MULT36X36 #(.AREG(1'b1), .BREG(1'b1), .PIPE_REG(1'b1),
        // OUT1 adds a stage to bits 71:18 only. Bypass it so the complete
        // 72-bit product has the same three-edge latency as its low 18 bits.
        .OUT0_REG(1'b1), .OUT1_REG(1'b0), .MULT_RESET_MODE("SYNC")) dsp (
        .A(a), .B(b), .ASIGN(1'b1), .BSIGN(1'b1), .CLK(clk),
        .CE(1'b1), .RESET(reset), .DOUT(product));
`endif
endmodule

module FrontendMvp #(parameter EXTERNAL_MULTIPLIER=0)(input wire clk, input wire reset, input wire start,
    input wire [127:0] position, input wire [9:0] matrix_address,
    output wire [9:0] scratch_address, input wire [63:0] scratch_data,
    output wire busy, output reg done, output reg error,
    output reg [127:0] clip,
    output wire signed [35:0] multiply_a,output wire signed [35:0] multiply_b,
    input wire signed [71:0] multiply_product);
    localparam IDLE=0, PRIME=1, ISSUE=2, FLUSH=3;
    reg [1:0] state;
    reg [9:0] base;
    reg [127:0] pos;
    reg [4:0] issue;
    reg [31:0] high_coefficient;
    reg [2:0] valid;
    reg [3:0] tag0,tag1,tag2;
    wire [31:0] coefficient = issue[0] ? high_coefficient : scratch_data[31:0];
    wire [31:0] component = pos[issue[1:0]*32+:32];
    wire signed [35:0] a={{4{coefficient[31]}},coefficient};
    wire signed [35:0] b={{4{component[31]}},component};
    wire signed [71:0] product;
    assign multiply_a=a;
    assign multiply_b=b;
    generate if(EXTERNAL_MULTIPLIER) begin: external_multiplier
        assign product=multiply_product;
    end else begin: internal_multiplier
        FrontendMultiply36 multiplier(clk,reset,a,b,product);
    end endgenerate
    reg signed [65:0] accumulator;
    wire signed [65:0] term=product[65:0];
    reg signed [65:0] total;
    reg signed [49:0] quotient;
    reg signed [50:0] rounded;
    always @* begin
        if (tag2[1:0]==0) total=term;
        else total=accumulator+term;
        quotient=total >>> 16;
        rounded={quotient[49],quotient};
        if (total[15:0]>16'h8000 || (total[15:0]==16'h8000 && quotient[0]))
            rounded=rounded+51'sd1;
    end
    assign scratch_address=(state==PRIME || state==IDLE) ? base : base + ((issue+2)>>1);
    assign busy=state!=IDLE;
    always @(posedge clk) begin
        done<=0;
        if (reset) begin state<=IDLE; valid<=0; issue<=0; error<=0;
            base<=0; pos<=0; high_coefficient<=0; accumulator<=0;
            tag0<=0;tag1<=0;tag2<=0;clip<=0;end
        else begin
            valid<={valid[1:0],state==ISSUE};
            tag0<=issue[3:0];tag1<=tag0;tag2<=tag1;
            case(state)
                IDLE: if(start) begin
                    base<=matrix_address;pos<=position;issue<=0;
                    error<=0;state<=PRIME;
                end
                // PRIME presents coefficient zero. It is consumed at the
                // first ISSUE edge after the synchronous RAM has returned it.
                PRIME: state<=ISSUE;
                ISSUE: begin
                    if(!issue[0]) high_coefficient<=scratch_data[63:32];
                    issue<=issue+1;
                    if(issue==15) state<=FLUSH;
                end
            endcase
            if(valid[2]) begin
                accumulator<=total;
                if(tag2[1:0]==3) begin
                    clip[tag2[3:2]*32+:32]<=rounded[31:0];
                    if(rounded[50:31]!={20{rounded[31]}}) error<=1;
                    if(tag2==15) begin state<=IDLE;done<=1;end
                end
            end
        end
    end
endmodule
