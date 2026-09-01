module tb;
	reg clk = 0;
	always #1 clk = ~clk;

	reg reset = 0;
	reg request_valid = 0;
	reg write = 0;
	reg read_line = 0;
	reg [21:0] address = 0;
	reg [15:0] write_data = 0;
	reg response_ready = 0;
	reg [31:0] controller_read_data = 0;
	reg controller_read_valid = 0;
	reg controller_init_done = 0;
	reg controller_command_ack = 0;
	wire request_ready;
	wire response_valid;
	wire [31:0] read_data;
	wire response_last;
	wire error;
	wire controller_command_valid;
	wire [2:0] controller_command;
	wire controller_precharge;
	wire [20:0] controller_address;
	wire [3:0] controller_write_mask;
	wire [31:0] controller_write_data;
	wire [7:0] controller_burst_length;

	TangNano20KSdramWordPort dut (.*);

	integer cycles = 0;
	always @(posedge clk) begin
		cycles <= cycles + 1;
		if (cycles > 20000)
			$fatal(1, "testbench cycle limit exceeded");
	end

	task acknowledge_command;
		input [2:0] expected;
		begin
			while (!controller_command_valid) @(posedge clk);
			if (controller_command !== expected) $fatal(1, "unexpected command");
			controller_command_ack <= 1;
			@(posedge clk);
			controller_command_ack <= 0;
		end
	endtask

	integer i;
	initial begin
		repeat (2) @(posedge clk);
		controller_init_done <= 1;
		while (!request_ready) @(posedge clk);
		address <= 22'h100007;
		write_data <= 16'habcd;
		write <= 1;
		request_valid <= 1;
		@(posedge clk);
		request_valid <= 0;
		acknowledge_command(3'b011);
		acknowledge_command(3'b100);
		if (controller_address !== 21'h080003 ||
			controller_write_mask !== 4'b0011 ||
			controller_write_data !== 32'habcd0000)
			$fatal(1, "physical address or upper lane mapping failed");
		while (!response_valid) @(posedge clk);
		if (!response_last) $fatal(1, "write completion must carry last");
		response_ready <= 1;
		@(posedge clk);
		response_ready <= 0;

		while (!request_ready) @(posedge clk);
		address <= 22'h000006;
		write <= 0;
		request_valid <= 1;
		@(posedge clk);
		request_valid <= 0;
		acknowledge_command(3'b011);
		fork
			acknowledge_command(3'b101);
			begin
				while (!(controller_command_valid && controller_command == 3'b101))
					@(posedge clk);
				@(posedge clk);
				controller_read_data <= 32'h56781234;
				controller_read_valid <= 1;
				@(posedge clk);
				controller_read_valid <= 0;
			end
		join
		while (!response_valid) @(posedge clk);
		if (read_data !== 32'h1234) $fatal(1, "lower lane read failed");
		if (!response_last) $fatal(1, "word read must carry last");
		response_ready <= 1;
		@(posedge clk);
		response_ready <= 0;
		if (error) $fatal(1, "unexpected adapter error");

		// One aligned line request must produce one READ burst command and
		// exactly eight ordered 32-bit response beats.
		while (!request_ready) @(posedge clk);
		address <= 22'h000120;
		read_line <= 1;
		request_valid <= 1;
		@(posedge clk);
		request_valid <= 0;
		read_line <= 0;
		acknowledge_command(3'b011);
		while (!(controller_command_valid && controller_command == 3'b101))
			@(posedge clk);
		if (controller_burst_length !== 8'd7)
			$fatal(1, "line read did not request an eight-beat burst");
		if (controller_address !== 21'h000090)
			$fatal(1, "line read address is not the burst base");
		controller_command_ack <= 1;
		@(posedge clk);
		controller_command_ack <= 0;
		for (i = 0; i < 8; i = i + 1) begin
			controller_read_data <= 32'h1000_0000 + i;
			controller_read_valid <= 1;
			@(posedge clk);
			#1;
			if (!response_valid) $fatal(1, "missing line beat %0d", i);
			if (read_data !== 32'h1000_0000 + i)
				$fatal(1, "bad line beat %0d", i);
			if (response_last !== (i == 7)) $fatal(1, "bad last at beat %0d", i);
		end
		controller_read_valid <= 0;
		@(posedge clk);
		#1;
		if (response_valid) $fatal(1, "line response did not end after beat seven");
		if (error) $fatal(1, "unexpected adapter error");

		// A word read after a line burst still works.
		while (!request_ready) @(posedge clk);
		address <= 22'h000006;
		request_valid <= 1;
		@(posedge clk);
		request_valid <= 0;
		acknowledge_command(3'b011);
		fork
			acknowledge_command(3'b101);
			begin
				while (!(controller_command_valid && controller_command == 3'b101))
					@(posedge clk);
				@(posedge clk);
				controller_read_data <= 32'h56781234;
				controller_read_valid <= 1;
				@(posedge clk);
				controller_read_valid <= 0;
			end
		join
		while (!response_valid) @(posedge clk);
		if (read_data !== 32'h1234) $fatal(1, "word read after line burst failed");
		if (!response_last) $fatal(1, "word read must carry last");
		response_ready <= 1;
		@(posedge clk);
		response_ready <= 0;

		$display("DIGITAL_DESIGN_PASS");
		$finish;
	end
endmodule
