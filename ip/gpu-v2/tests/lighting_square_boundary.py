"""Bounded selected-core replay and post-PnR split-square register audit."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def module(source, name):
    return re.search(r"module " + name + r"\s*\(.*?endmodule", source, re.S)[0]


def bits(expression):
    result = []
    for term in expression.strip().strip("{}").split(","):
        term = term.strip()
        match = re.fullmatch(r"(\\\S+)\s*\[(\d+)(?::(\d+))?\]", term)
        if match:
            name, high, low = match.groups()
            high, low = int(high), int(low or high)
            result.extend((name, bit) for bit in range(high, low - 1, -1))
        else:
            result.append((term, None))
    return result


def audit(netlist):
    source = netlist.read_text()
    registered = set()
    for cell in re.finditer(r"(?m)^DFF\w*\s+\\\S+\s+\((.*?)\n\);", source, re.S):
        q = re.search(r"\.Q\((.*?)\)", cell[1], re.S)
        if q:
            registered.update(bits(q[1]))
    # Preserve aliases as well: a kept tail net may alias the original Q bus.
    aliases = re.findall(r"(?m)^assign\s+(.*?)\s*=\s*(.*?);", source)
    for _ in range(4):
        for left, right in aliases:
            lhs, rhs = bits(left), bits(right)
            if len(lhs) == len(rhs):
                registered.update(a for a, b in zip(lhs, rhs) if b in registered)
    results = []
    for cell in re.finditer(
        r"MULT9X9\s+(\\\S+/dsp(?:24|28))\s+\((.*?)\n\);", source, re.S
    ):
        name, body = cell.groups()
        port = re.search(r"\.B\((.*?)\),", body, re.S)[1]
        tail = bits(port)[-7:]
        ok = len(tail) == 7 and all(bit in registered for bit in tail)
        results.append({"cell": name, "B": port, "registered_tail": ok})
    return {
        "netlist_sha256": hashlib.sha256(netlist.read_bytes()).hexdigest(),
        "passed": len(results) >= 2 and all(item["registered_tail"] for item in results),
        "cells": results,
    }


BENCH = r"""`timescale 1ns/1ps
module tb;
GSR GSR(.GSRI(1'b1));reg clk=0;always #5 clk=~clk;
reg reset=1;integer cycles=0,sent=0,received=0;
wire ce=cycles%13<11;wire ready,valid;wire[8:0]g,h;wire[31:0]id;
wire offer=sent<12&&cycles>=8;
reg[35:0]r0=36'h02710f5e0,r1=36'h0fcf62b80,r2=36'h000001c00;reg[8:0]eg,eh;
reg signed[15:0]lx,ly,lz,rx,ry,rk;reg[8:0]ia,di;reg[4:0]code;reg[1:0]mode;
always@*case(sent%3)
0:begin r0=36'h02710f5e0;r1=36'h0fcf62b80;r2=36'h000001c00;end
1:begin r0=36'h014300b90;r1=36'h0ffd71a80;r2=36'h000001b77;end
2:begin r0=36'h020d0f880;r1=36'h0fe3d3150;r2=36'h000001aef;end
endcase
always@*case(id%3)
0:begin eg=158;eh=0;end
1:begin eg=223;eh=8;end
2:begin eg=159;eh=0;end
endcase
gpu_v2_lighting dut(.clk(clk),.reset(reset),.ce(ce),.context_epoch(16'd0),
.context_mode(mode),.context_code(code),.light_x(lx),.light_y(ly),.light_z(lz),
.ambient(ia),.directional(di),.ray_x(rx),.ray_y(ry),.ray_k(rk),
.in_valid(offer),.in_ready(ready),.in_id(sent),.in_row0(r0),.in_row1(r1),.in_row2(r2),
.out_valid(valid),.out_ready(1'b1),.out_id(id),.out_epoch(),.out_g(g),.out_h(h));
always@(posedge clk)begin
if(cycles>1000)$fatal(1,"bounded replay");
if(!reset&&ce)begin
 if(valid)begin
  if(id!==received||g!==eg||h!==eh)$fatal(1,"id=%0d g/h=%0d/%0d expected=%0d/%0d",id,g,h,eg,eh);
  received=received+1;
  if(received==12)begin $display("PASS split-square tokens=12 pauses=enabled");$finish;end
 end
end
end
always@(negedge clk)cycles=cycles+1;
// Advance the source after the accepted edge, avoiding a testbench race.
always@(posedge clk)if(!reset&&ready&&offer)begin #1;sent=sent+1;end
initial begin
lx=0;ly=0;lz=0;rx=0;ry=0;rk=0;ia=0;di=0;code=0;mode=0;
repeat(3)@(negedge clk);reset=0;
lx=12534;ly=8667;lz=6018;rx=-8192;ry=-4915;rk=8192;ia=64;di=192;code=12;mode=3;
end
endmodule
"""


def replay(rtl, output):
    output.mkdir(parents=True, exist_ok=True)
    source = rtl.read_text()
    (output / "core.v").write_text(
        module(source, "gpu_v2_lighting") + "\n" + module(source, "lighting_norm_q13")
    )
    (output / "tb.v").write_text(BENCH)
    library = Path(os.environ["GOWIN_HOME"]) / "IDE/simlib/gw2a/prim_sim.v"
    commands = [
        [os.environ["IVERILOG_EXE"], "-g2012", "-DGPU_V2_GOWIN_DSP", "-s", "tb", "-o",
         str(output / "sim.vvp"), str(output / "core.v"), str(output / "tb.v"), str(library)],
        [os.environ["VVP_EXE"], str(output / "sim.vvp")],
    ]
    for label, command in zip(["compile", "replay"], commands):
        result = subprocess.run(command, capture_output=True, text=True, timeout=60)
        (output / (label + ".log")).write_text(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError(f"{label} failed; see {output / (label + '.log')}")
    return {"passed": True, "rtl_sha256": hashlib.sha256(rtl.read_bytes()).hexdigest(),
            "tokens": 12, "cycle_limit": 1000}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--netlist", type=Path)
    parser.add_argument("--rtl", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if bool(args.netlist) == bool(args.rtl):
        parser.error("select exactly one of --netlist and --rtl")
    args.output.mkdir(parents=True, exist_ok=True)
    receipt = audit(args.netlist) if args.netlist else replay(args.rtl, args.output)
    (args.output / "receipt.json").write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt))
    raise SystemExit(0 if receipt["passed"] else 1)
