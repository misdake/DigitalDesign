integer audit_collisions=0,audit_disabled_b_matches=0;
always @(posedge clk) begin
 audit_taint_a<=write_enable && write_address==read_a_address;
 if(read_b_enable)audit_taint_b<=write_enable && write_address==read_b_address;
 if(write_enable && write_address==read_a_address)audit_collisions=audit_collisions+1;
 if(write_enable && write_address==read_b_address)begin
  if(read_b_enable)audit_collisions=audit_collisions+1;
  else audit_disabled_b_matches=audit_disabled_b_matches+1;
 end
end
endmodule
