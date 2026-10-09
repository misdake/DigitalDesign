module tb;
reg clk = 0;
reg write_enable = 0, write_way = 0;
reg [5:0] address = 0;
reg [11:0] write_data = 0;
wire [11:0] way_0_read_data, way_1_read_data;
reg [11:0] golden [0:127];
reg [11:0] expected_0 = 0, expected_1 = 0;
reg [31:0] random_state = 32'h7381ab25;
integer i;
CpuV3CacheTagBsram dut(.*);
always #5 clk = ~clk;
initial begin
    for(i=0;i<128;i=i+1) golden[i]=0;
    // Visit every set and way, then mix reads and writes to exercise address,
    // way isolation, all twelve tag bits and normal-mode write output hold.
    for(i=0;i<2048;i=i+1) begin
        @(negedge clk);
        random_state = (random_state ^ (random_state << 13));
        random_state = (random_state ^ (random_state >> 17));
        random_state = (random_state ^ (random_state << 5));
        address = i<128 ? i%64 : random_state[5:0];
        write_way = i<128 ? i/64 : random_state[6];
        write_enable = i<128 || random_state[7];
        write_data = random_state[19:8];
        if(write_enable && !write_way) golden[address]=write_data;
        else expected_0=golden[address];
        if(write_enable && write_way) golden[64+address]=write_data;
        else expected_1=golden[64+address];
        @(posedge clk); #1;
        if(way_0_read_data !== expected_0 || way_1_read_data !== expected_1)
            $fatal(1,"tag synchronous read/write mismatch cycle=%0d set=%0d",i,address);
    end
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
initial begin #21000; $fatal(1,"tag test timeout"); end
endmodule
