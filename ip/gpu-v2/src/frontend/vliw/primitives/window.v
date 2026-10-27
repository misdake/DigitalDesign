// 四相固定来源：D32 / H0 / LOD abs36 / H1；没有请求或仲裁。
module iz_window_source(input[1:0]phase,input[31:0]d,h,input[35:0]lod_abs,output reg[35:0]value);
 always @*begin
  case(phase)
   0:value={4'd0,d};
   2:value=lod_abs;
   default:value={4'd0,h};
  endcase
 end
endmodule

// 一个53→18窗口；D和LOD自动leading，H按保存的D指数+1取窗。
// 公共mant旧Q依次在phi1/2/3/0消费，绝不再复制三份结果缓存。
module iz_four_phase_window(input clk,reset,ce,input[1:0]phase,input d_valid,h_valid,
 input[31:0]d,h,input lod_input_valid,input signed[35:0]lod_input_value,
 output reg[17:0]mant,output reg[4:0]d_exponent,
 output reg lod_valid,lod_zero,output reg[5:0]lod_exponent,output lod_pending);
 reg[35:0]lod_abs;reg abs_valid;
 wire[35:0]negated=36'd0-lod_input_value;
 wire[35:0]absolute_value=lod_input_value[35]?negated:lod_input_value;
 wire[35:0]value;
 iz_window_source source_select(phase,d,h,lod_abs,value);
 wire automatic_window=!phase[0];
 wire[5:0]h_shift={1'b0,d_exponent}+6'd1;
 wire[52:0]window0={value,17'd0};
 wire choose32=automatic_window ? (|value[35:32]) : h_shift[5];
 wire[48:0]window32=choose32 ? {28'd0,window0[52:32]} : window0[48:0];
 wire choose16=automatic_window ? (|window32[48:33]) : h_shift[4];
 wire[32:0]window16=choose16 ? window32[48:16] : window32[32:0];
 wire choose8=automatic_window ? (|window16[32:25]) : h_shift[3];
 wire[24:0]window8=choose8 ? window16[32:8] : window16[24:0];
 wire choose4=automatic_window ? (|window8[24:21]) : h_shift[2];
 wire[20:0]window4=choose4 ? window8[24:4] : window8[20:0];
 wire choose2=automatic_window ? (|window4[20:19]) : h_shift[1];
 wire[18:0]window2=choose2 ? window4[20:2] : window4[18:0];
 wire choose1=automatic_window ? window2[18] : h_shift[0];
 wire[17:0]window1=choose1 ? window2[18:1] : window2[17:0];
 wire[5:0]leading={choose32,choose16,choose8,choose4,choose2,choose1};
 wire take_d=phase==0&&d_valid;
 wire take_h=phase[0]&&h_valid;
 wire take_lod=phase==2&&abs_valid;
 assign lod_pending=abs_valid||lod_valid;
 always @(posedge clk)begin
  if(reset)begin mant<=0;d_exponent<=0;lod_abs<=0;abs_valid<=0;lod_valid<=0;lod_zero<=0;lod_exponent<=0;end
  else if(ce)begin
   abs_valid<=lod_input_valid;
   if(lod_input_valid)lod_abs<=absolute_value;
   lod_valid<=take_lod;
   if(take_d||take_h||take_lod)mant<=window1;
   if(take_d)d_exponent<=leading[4:0];
   if(take_lod)begin lod_exponent<=leading;lod_zero<=value==0;end
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset)begin
  if(lod_input_valid&&phase!=1)$fatal(1,"LOD abs不在固定phi1");
  if(abs_valid&&phase!=2)$fatal(1,"LOD取窗不在固定phi2");
 end
`endif
endmodule
