// Closed ordered public result owner; Context8 is owned by the parent.
module gpu_v2_texture_runtime_color(
 input wire clk,
 input wire reset,
 input wire proposal_ce,
 input wire commit_allow,
 input wire [3:0] check_quad,
 output wire public_free,
 input wire publish_valid,
 input wire [3:0] publish_quad,
 input wire [3:0] publish_mask,
 output wire publish_ready,
 output wire publish_fire,
 input wire in_valid,
 input wire [71:0] in_payload,
 input wire [63:0] in_texels,
 output wire in_ready,
 output wire input_fire,
 input wire out_ready,
 output wire out_valid,
 output wire [5:0] out_key,
 output wire [23:0] out_rgb,
 output wire quad_last,
 output wire result_fire,
 output wire eligible,
 output wire advance,
 output wire preflight_fault,
 output wire fault,
 output wire [15:0] public_live
);
 reg terminal;
 reg [15:0] live;
 reg [1:0] last_lane [0:15];
 wire color_fault,color_bad,color_ready,color_valid;
 wire [5:0] color_key;
 wire [7:0] color_r,color_g,color_b;
 wire fault_eff=terminal || color_fault;
 wire attempted=proposal_ce && !reset && !fault_eff;
 // These are old-state credits. A final pop cannot fund a same-edge alias.
 assign public_free=!reset && !fault_eff && !live[check_quad];
 assign publish_ready=!reset && !fault_eff && !live[publish_quad] && (publish_mask!=0);
 wire publish_bad=publish_valid && !publish_ready;
 wire input_proposal=in_valid && color_ready;
 wire input_bad=input_proposal && !live[in_payload[69:66]];
 wire pop_proposal=color_valid && out_ready;
 wire result_bad=pop_proposal && !live[color_key[5:2]];
 assign preflight_fault=attempted && (publish_bad || input_bad || result_bad || color_bad);
 assign eligible=attempted && !preflight_fault;
 assign advance=eligible && commit_allow;
 // Terminal status and raw offers do not depend on this edge's proposal bad.
 // The parent unions preflight separately before committing the coalition.
 assign fault=fault_eff;
 assign in_ready=color_ready && !reset && !fault_eff;
 assign input_fire=advance && in_valid && color_ready;
 assign publish_fire=advance && publish_valid && publish_ready;
 assign out_valid=!reset && !fault_eff && color_valid;
 assign out_key=color_key;
 assign out_rgb={color_b,color_g,color_r};
 assign quad_last=out_valid && live[color_key[5:2]] && (color_key[1:0]==last_lane[color_key[5:2]]);
 assign result_fire=advance && out_valid && out_ready;
 assign public_live=live;
 gpu_v2_texture_runtime_color_checked color(
  .clk(clk),.reset(reset),.ce(advance),.proposal_ce(attempted),.preflight_bad(color_bad),
  .in_valid(in_valid),.in_payload(in_payload),
  .in_tex0(in_texels[15:0]),.in_tex1(in_texels[31:16]),
  .in_tex2(in_texels[47:32]),.in_tex3(in_texels[63:48]),
  .in_ready(color_ready),.out_ready(out_ready),.out_valid(color_valid),
  .out_key(color_key),.out_r(color_r),.out_g(color_g),.out_b(color_b),.fault(color_fault));
 always @(posedge clk) begin
  if(reset) begin terminal<=0;live<=0;end
  else begin
   if(preflight_fault) terminal<=1;
   if(advance) begin
    if(result_fire && quad_last) live[color_key[5:2]]<=0;
    if(publish_fire) begin
     live[publish_quad]<=1;
     if(publish_mask[3]) last_lane[publish_quad]<=3;
     else if(publish_mask[2]) last_lane[publish_quad]<=2;
     else if(publish_mask[1]) last_lane[publish_quad]<=1;
     else last_lane[publish_quad]<=0;
    end
   end
  end
 end
endmodule
