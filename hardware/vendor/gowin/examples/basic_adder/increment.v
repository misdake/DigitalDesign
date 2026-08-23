module Increment6(
    input wire [5:0] value,
    output wire [5:0] incremented
);

assign incremented = value + 1'b1;

endmodule
