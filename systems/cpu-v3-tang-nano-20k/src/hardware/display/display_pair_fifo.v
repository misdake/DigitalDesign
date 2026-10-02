// Completed RGB888 pairs: bits 23:0 are the first pixel, 47:24 the second.
// The parent owns occupancy and wrapping pointers; this leaf is only storage.
// Its asynchronous head read supplies the first output pixel on the pop edge.
// Do not reset the array: row/reset handling invalidates it through occupancy.
// Only four of each RAM16's sixteen addresses are used. The 48-bit read width
// still requires twelve 16x4 cells, so reducing logical depth alone saves none.
//
// Keep the separate leaf and parallel pair access unless a COMPLETE system fit
// improves. Direct handoff and a serialized narrower FIFO were functionally
// correct and locally smaller, but remapped unrelated logic (especially the
// SDRAM adapter) and increased the default 2x system total. This is a measured
// synthesis tradeoff, not evidence that the extra RAM improves adapter behavior.
// See docs/cpu-v3-optimization-record.md, "Display four-row buffering and
// pixel-pair FIFO", for the comparison and verification boundary.
module DisplayPairFifo(
    input wire write_clock, input wire write_enable,
    input wire [1:0] write_address, input wire [47:0] write_data,
    input wire [1:0] read_address, output wire [47:0] read_data
);
(* syn_ramstyle = "distributed_ram" *) reg [47:0] memory [0:3];
always @(posedge write_clock)
    if (write_enable) memory[write_address]<=write_data;
assign read_data=memory[read_address];
endmodule
