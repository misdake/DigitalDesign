// Correctness-first triangle front end. A tiny LUT ROM selects one of three
// existing arithmetic kernels. Only the selected kernel owns the registered
// 36x36 multiplier; a stage change occurs after its result pipeline drains.
// One source triangle is accepted at a time. The complete clipped fan is
// buffered before setup, so geometry and setup never contend for the DSP.
module GpuGeometryMicrocore #(parameter SHARED_MULTIPLIER=1)(input wire clk,input wire reset,
    input wire start_valid,output wire start_ready,
    input wire [383:0] input_positions,input wire [47:0] input_rgb565,
    input wire [9:0] matrix_address,input wire source_last,
    output wire [9:0] scratch_address,input wire [63:0] scratch_data,
    output wire output_valid,input wire output_ready,output wire output_end,
    output wire output_source_last,output wire output_error,
    output reg [95:0] output_xy,output reg [707:0] output_planes,
    output reg [5:0] output_scale);

localparam [3:0] PC_IDLE=0,PC_MVP_START=1,PC_MVP_WAIT=2,
    PC_GEOM_FEED=3,PC_GEOM_DRAIN=4,PC_SETUP_START=5,
    PC_SETUP_WAIT=6,PC_TRI_OUT=7,PC_SOURCE_END=8;
localparam [2:0] OP_IDLE=0,OP_MVP_START=1,OP_MVP_WAIT=2,
    OP_GEOM_FEED=3,OP_GEOM_DRAIN=4,OP_SETUP_START=5,
    OP_SETUP_WAIT=6,OP_OUTPUT=7;
reg [3:0] pc;
reg [2:0] opcode;
// This small control store deliberately maps to logic ROM. Branches wait for
// ready/done or for the variable number of clipped fan triangles.
always @* begin
    case(pc)
        PC_MVP_START:opcode=OP_MVP_START;
        PC_MVP_WAIT:opcode=OP_MVP_WAIT;
        PC_GEOM_FEED:opcode=OP_GEOM_FEED;
        PC_GEOM_DRAIN:opcode=OP_GEOM_DRAIN;
        PC_SETUP_START:opcode=OP_SETUP_START;
        PC_SETUP_WAIT:opcode=OP_SETUP_WAIT;
        PC_TRI_OUT,PC_SOURCE_END:opcode=OP_OUTPUT;
        default:opcode=OP_IDLE;
    endcase
end

reg [383:0] positions;
reg [47:0] colors;
reg [9:0] matrix_base;
reg last;
reg error;
reg [1:0] vertex_index;
reg [4:0] projected_count,setup_index;
reg [127:0] clip0,clip1,clip2;
reg [103:0] projected [0:23];

wire [127:0] mvp_position=positions[vertex_index*128+:128];
wire mvp_busy,mvp_done,mvp_error;
wire [127:0] mvp_clip;
wire signed [35:0] mvp_a,mvp_b,geo_a,geo_b,setup_a,setup_b;
wire signed [71:0] product;
wire mvp_start=opcode==OP_MVP_START;
FrontendMvp #(.EXTERNAL_MULTIPLIER(SHARED_MULTIPLIER)) mvp(
    .clk(clk),.reset(reset),.start(mvp_start),.position(mvp_position),
    .matrix_address(matrix_base),.scratch_address(scratch_address),
    .scratch_data(scratch_data),.busy(mvp_busy),.done(mvp_done),
    .error(mvp_error),.clip(mvp_clip),.multiply_a(mvp_a),
    .multiply_b(mvp_b),.multiply_product(product));

wire [127:0] selected_clip=vertex_index==0 ? clip0 :
    vertex_index==1 ? clip1 : clip2;
wire [15:0] selected_color=colors[vertex_index*16+:16];
wire [207:0] geometry_vertex={28'd0,selected_color,12'h400,24'd0,selected_clip};
wire geometry_input_ready,geometry_output_valid,geometry_output_end;
wire geometry_output_last,geometry_error;
wire [31:0] geometry_xy,geometry_w;
wire [47:0] geometry_color;
wire [23:0] geometry_invw;
wire geometry_input_valid=opcode==OP_GEOM_FEED;
wire geometry_output_ready=opcode==OP_GEOM_DRAIN;
wire geometry_reset=reset || (start_valid && start_ready);
GpuColorGeometry #(.EXTERNAL_MULTIPLIER(SHARED_MULTIPLIER)) geometry(
    .clk(clk),.reset(geometry_reset),.input_valid(geometry_input_valid),
    .input_ready(geometry_input_ready),.input_vertex(geometry_vertex),
    .input_source_last(last && vertex_index==2),
    .output_valid(geometry_output_valid),.output_ready(geometry_output_ready),
    .output_end(geometry_output_end),.output_source_last(geometry_output_last),
    .output_xy(geometry_xy),.output_color(geometry_color),
    .output_w(geometry_w),.output_invw(geometry_invw),.error(geometry_error),
    .external_multiply_a(geo_a),.external_multiply_b(geo_b),
    .external_multiply_product(product));

wire [103:0] projected0=projected[setup_index];
wire [103:0] projected1=projected[setup_index+5'd1];
wire [103:0] projected2=projected[setup_index+5'd2];
wire [95:0] setup_xy={projected2[31:0],projected1[31:0],projected0[31:0]};
wire [143:0] setup_colors={projected2[79:32],projected1[79:32],projected0[79:32]};
wire [71:0] setup_inverse={projected2[103:80],projected1[103:80],projected0[103:80]};
wire setup_input_ready,setup_output_valid;
wire [707:0] setup_planes;
wire [5:0] setup_scale;
wire setup_input_valid=opcode==OP_SETUP_START;
wire setup_output_ready=opcode==OP_SETUP_WAIT;
GpuColorSetup #(.EXTERNAL_MULTIPLIER(SHARED_MULTIPLIER)) setup(
    .clk(clk),.reset(reset),.input_valid(setup_input_valid),
    .input_ready(setup_input_ready),.input_xy(setup_xy),
    .input_color(setup_colors),.input_invw(setup_inverse),
    .output_valid(setup_output_valid),.output_ready(setup_output_ready),
    .output_planes(setup_planes),.output_scale(setup_scale),
    .external_multiply_a(setup_a),.external_multiply_b(setup_b),
    .external_multiply_product(product));

reg signed [35:0] multiplier_a,multiplier_b;
always @* begin
    multiplier_a=0;multiplier_b=0;
    case(pc)
        PC_MVP_START,PC_MVP_WAIT:begin multiplier_a=mvp_a;multiplier_b=mvp_b;end
        PC_GEOM_FEED,PC_GEOM_DRAIN:begin multiplier_a=geo_a;multiplier_b=geo_b;end
        PC_SETUP_START,PC_SETUP_WAIT:begin multiplier_a=setup_a;multiplier_b=setup_b;end
    endcase
end
generate if(SHARED_MULTIPLIER) begin: shared_multiplier
    FrontendMultiply36 multiplier(clk,reset,multiplier_a,multiplier_b,product);
end else begin: separate_multipliers
    assign product=0;
end endgenerate

assign start_ready=pc==PC_IDLE;
assign output_valid=pc==PC_TRI_OUT || pc==PC_SOURCE_END;
assign output_end=pc==PC_SOURCE_END;
assign output_source_last=last;
assign output_error=error || geometry_error;
always @(posedge clk) begin
    if(reset) begin
        pc<=PC_IDLE;positions<=0;colors<=0;matrix_base<=0;last<=0;
        error<=0;vertex_index<=0;projected_count<=0;setup_index<=0;
        clip0<=0;clip1<=0;clip2<=0;output_xy<=0;output_planes<=0;output_scale<=0;
    end else begin
        case(opcode)
            OP_IDLE:if(start_valid) begin
                positions<=input_positions;colors<=input_rgb565;
                matrix_base<=matrix_address;last<=source_last;error<=0;
                vertex_index<=0;projected_count<=0;setup_index<=0;
                pc<=PC_MVP_START;
            end
            OP_MVP_START:pc<=PC_MVP_WAIT;
            OP_MVP_WAIT:if(mvp_done) begin
                case(vertex_index)
                    0:clip0<=mvp_clip;
                    1:clip1<=mvp_clip;
                    default:clip2<=mvp_clip;
                endcase
                if(mvp_error) begin error<=1;pc<=PC_SOURCE_END;end
                else if(vertex_index==2) begin vertex_index<=0;pc<=PC_GEOM_FEED;end
                else begin vertex_index<=vertex_index+1'b1;pc<=PC_MVP_START;end
            end
            OP_GEOM_FEED:if(geometry_input_ready) begin
                if(vertex_index==2) pc<=PC_GEOM_DRAIN;
                else vertex_index<=vertex_index+1'b1;
            end
            OP_GEOM_DRAIN:if(geometry_output_valid) begin
                if(geometry_output_end) begin
                    if(projected_count==0) pc<=PC_SOURCE_END;
                    else begin setup_index<=0;pc<=PC_SETUP_START;end
                end else if(projected_count<24) begin
                    projected[projected_count]<={geometry_invw,geometry_color,geometry_xy};
                    projected_count<=projected_count+1'b1;
                end else error<=1;
            end
            OP_SETUP_START:if(setup_input_ready) pc<=PC_SETUP_WAIT;
            OP_SETUP_WAIT:if(setup_output_valid) begin
                output_xy<=setup_xy;output_planes<=setup_planes;
                output_scale<=setup_scale;pc<=PC_TRI_OUT;
            end
            OP_OUTPUT:if(output_ready) begin
                if(pc==PC_SOURCE_END) pc<=PC_IDLE;
                else if(setup_index+5'd3<projected_count) begin
                    setup_index<=setup_index+5'd3;pc<=PC_SETUP_START;
                end else pc<=PC_SOURCE_END;
            end
        endcase
    end
end
endmodule
