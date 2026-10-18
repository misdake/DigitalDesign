// Scalar old-C stage 1 and live old-D stage 2; unchanged selected ROP math.
module gpu_v2_rop_tdp (
    input wire[53:0]shared_rop_product,output wire shared_mul_pending,shared_rop_ce,output wire[26:0]shared_rop_a,output wire[7:0]shared_rop_b,
    input wire shade_phase,
    input             clk,
    input             reset,
    input             ce,
    input             in_valid,
    input             blend,        // 0 = REPLACE, 1 = SRC_OVER
    input      [2:0]  depth_func,
    input             depth_write,
    input             covered,
    input      [15:0] old_color,
    input      [15:0] old_depth,
    input      [7:0]  src_r,
    input      [7:0]  src_g,
    input      [7:0]  src_b,
    input      [7:0]  src_a,
    input      [15:0] src_depth,
    output wire       in_ready,
    output reg        out_valid,
    output reg [15:0] new_color,
    output wire [15:0] new_depth,
    output reg        color_written,
    output reg        depth_written
);
    // No output FIFO and no backpressure: one lane is accepted per enabled edge.
    assign in_ready = ce;

    function [4:0] exactq5;
        input [7:0] c;
        reg [4:0] head;
        reg [2:0] low;
        reg down,up;
        begin
            head=c[7:3];low=c[2:0];
            down=((low==3'd0)&&(head>=5'd19))||((low==3'd1)&&(head>=5'd23))||((low==3'd2)&&(head>=5'd28));
            up=((low==3'd5)&&(head<=5'd3))||((low==3'd6)&&(head<=5'd8))||((low==3'd7)&&(head<=5'd12));
            exactq5=head+{4'd0,up}-{4'd0,down};
        end
    endfunction

    function [5:0] exactq6;
        input [7:0] c;
        reg [5:0] head;
        reg [1:0] low;
        reg down,up;
        begin
            head=c[7:2];low=c[1:0];
            down=(low==2'd0)&&(head>=6'd43);
            up=(low==2'd3)&&(head<=6'd20);
            exactq6=head+{5'd0,up}-{5'd0,down};
        end
    endfunction

    function [7:0] expand5;
        input [4:0] c;
        begin
            expand5 = {c, c[4:2]};
        end
    endfunction

    function [7:0] expand6;
        input [5:0] c;
        begin
            expand6 = {c, c[5:4]};
        end
    endfunction

    function depth_pass;
        input [2:0]  f;
        input [15:0] s;
        input [15:0] o;
        begin
            case (f)
                3'd0: depth_pass = 1'b0;
                3'd1: depth_pass = (s < o);
                3'd2: depth_pass = (s == o);
                3'd3: depth_pass = (s <= o);
                3'd4: depth_pass = (s > o);
                3'd5: depth_pass = (s != o);
                3'd6: depth_pass = (s >= o);
                default: depth_pass = 1'b1;
            endcase
        end
    endfunction

    // Four enabled edges, one pixel every two shared phase edges.
    reg v1,v2,v3;
    reg[15:0]sd1,sd3,oc1;
    assign new_depth=sd3;
    reg[7:0]sr1,sg1,sb1,sa1;
    reg covered1,dw1,blend1;reg[2:0]func1;
    reg gate2,dw2;
    reg[7:0]dr2,dg2,db2;
    reg[7:0]a2;
    reg signed[8:0]delta_r2,delta_g2,delta_b2;
    wire[17:0]delta_r3=shared_rop_product[17:0],delta_g3=shared_rop_product[35:18],delta_b3=shared_rop_product[53:36];
    assign shared_mul_pending=v2;assign shared_rop_ce=ce&&v2;assign shared_rop_a={delta_b2,delta_g2,delta_r2};assign shared_rop_b=a2;
    // The BSRAM A read of D was issued on the preceding stage-1 edge.
    wire[15:0]compared_old_depth=old_depth;
    wire[8:0]alpha_sum={1'b0,sa1}+9'd2;
    wire[13:0]nr3={dr2,6'd0}+delta_r3[13:0]+14'd32;
    wire[13:0]ng3={dg2,6'd0}+delta_g3[13:0]+14'd32;
    wire[13:0]nb3={db2,6'd0}+delta_b3[13:0]+14'd32;
    always @(posedge clk)begin
      if(reset)begin
        v1<=0;v2<=0;v3<=0;
        out_valid<=0;new_color<=0;color_written<=0;depth_written<=0;
      end else if(ce)begin
        v1<=in_valid;if(in_valid)begin sd1<=src_depth;oc1<=old_color;end
        sr1<=src_r;sg1<=src_g;sb1<=src_b;sa1<=src_a;
        covered1<=covered;dw1<=depth_write;blend1<=blend;func1<=depth_func;
        v2<=v1;
        // Phase0 captures once; phase1 borrows these values. The next
        // phase0 consumes the old values before accepting the next pixel.
        if(!shade_phase)begin
          gate2<=covered1&&depth_pass(func1,sd1,compared_old_depth);dw2<=dw1;
          dr2<=expand5(oc1[15:11]);dg2<=expand6(oc1[10:5]);db2<=expand5(oc1[4:0]);
        end
        a2<=blend1?alpha_sum[8:2]:7'd64;
        delta_r2<=$signed({1'b0,sr1})-$signed({1'b0,expand5(oc1[15:11])});
        delta_g2<=$signed({1'b0,sg1})-$signed({1'b0,expand6(oc1[10:5])});
        delta_b2<=$signed({1'b0,sb1})-$signed({1'b0,expand5(oc1[4:0])});
        v3<=v2;if(shade_phase)sd3<=sd1;
                out_valid<=v3;
        color_written<=gate2;depth_written<=gate2&&dw2;
        new_color<={exactq5(nr3[13:6]),exactq6(ng3[13:6]),exactq5(nb3[13:6])};
      end
    end

endmodule
