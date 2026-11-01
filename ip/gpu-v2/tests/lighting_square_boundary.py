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
    """Audit the selected Gowin mapped shape, including each operand's actual Q owner."""
    source = netlist.read_text()
    cells = []
    for cell in re.finditer(r"(?m)^(\w+)\s+(\S+)\s+\((.*?)\n\);", source, re.S):
        kind, name, body = cell.groups()
        ports = dict(re.findall(r"\.(\w+)\((.*?)\)", body, re.S))
        cells.append((kind, name, ports))
    aliases = {}
    for left, right in re.findall(r"(?m)^assign\s+(.*?)\s*=\s*(.*?);", source):
        lhs, rhs = bits(left), bits(right)
        if len(lhs) == len(rhs):
            aliases.update(zip(lhs, rhs))

    def resolve(bit):
        visited = set()
        while bit in aliases and bit not in visited:
            visited.add(bit)
            bit = aliases[bit]
        return bit

    def port(ports, name):
        return [resolve(bit) for bit in bits(ports[name])]

    registers = {}
    producer_q = {}
    for kind, name, ports in cells:
        if kind.startswith("DFF"):
            for q in port(ports, "Q"):
                registers[q] = (name, port(ports, "CLK"))
                owner = re.fullmatch(r"(.*)/unit(11|12)_p0_(\d+)(?:_s\d+)?", name)
                if owner:
                    scope, producer, index = owner.groups()
                    producer_q[(scope, int(producer), int(index))] = q
    parameters = {}
    for name, parameter, value in re.findall(
        r"(?m)^defparam\s+(\S+)\s+\.(\w+)\s*=\s*([^;]+);", source
    ):
        parameters.setdefault(name, {})[parameter] = value.strip()
    results = []
    scopes = set()
    for kind, name, ports in cells:
        match = re.search(r"/dsp(24|28)$", name)
        if kind != "MULT9X9" or not match:
            continue
        consumer = int(match[1])
        producer = 12 if consumer == 24 else 11
        scope = name.rsplit("/", 1)[0]
        scopes.add(scope)
        # Gowin can name the low Q pins after the kept tail alias. The
        # preserved FF instance, not an arbitrary register net, owns each bit.
        def q(index):
            return producer_q.get((scope, producer, index),
                                  (f"missing:{scope}/unit{producer}_p0", index))
        # Selected correction is (high8,1) * zero-extended low7. Its signed
        # high bits, sign and low bits must all belong to this prescaled Q.
        a, b = port(ports, "A"), port(ports, "B")
        high = [q(i) for i in range(14, 6, -1)]
        low = [q(i) for i in range(6, -1, -1)]
        whole_q = [q(i) for i in range(16)]
        clock = port(ports, "CLK")
        same_q = (a == high + [("VCC", None)]
                  and b == [("GND", None)] * 2 + low
                  and port(ports, "ASIGN") == [q(14)]
                  and all(bit in registers and registers[bit][1] == clock for bit in whole_q))
        params = parameters.get(name, {})
        same_q = same_q and params.get("OUT_REG") == "1'b1" and all(
            params.get(p) == "1'b0" for p in ["AREG", "BREG", "PIPE_REG", "ASIGN_REG", "BSIGN_REG"])
        results.append({"cell": name, "producer": producer, "consumer": consumer, "A": ports["A"],
                        "B": ports["B"], "same_prescaled_q": same_q,
                        "preserved_q_bits": sum(bit in registers for bit in whole_q),
                        "clock": clock, "ce": port(ports, "CE")})
    macs = []
    for kind, name, ports in cells:
        if kind != "MULTADDALU18X18" or name.rsplit("/", 1)[0] not in scopes:
            continue
        match = re.search(r"/dsp(\d+)$", name)
        if not match:
            continue
        params = parameters.get(name, {})
        stage = params.get("OUT_REG") == "1'b1" and all(
            params.get(p) == "1'b0" for p in
            ["A0REG", "B0REG", "A1REG", "B1REG", "CREG", "PIPE0_REG", "PIPE1_REG",
             "ASIGN0_REG", "ASIGN1_REG", "BSIGN0_REG", "BSIGN1_REG"])
        scope = name.rsplit("/", 1)[0]
        tail = f"{scope}/unit{match[1]}_tail"
        fabric_tail = (any(q[0] == tail for q in registers)
                       or any(kind.startswith("DFF") and re.search(
                           rf"{re.escape(scope)}/unit{match[1]}_(?:tail|result|p\d+)(?:_|$)", cell)
                           for kind, cell, _ in cells))
        paired = [r for r in results if r["cell"].rsplit("/", 1)[0] == scope]
        same_enable = all(r["clock"] == port(ports, "CLK") and r["ce"] == port(ports, "CE")
                          for r in paired)
        macs.append({"cell": name, "hard_output_only": stage and not fabric_tail,
                     "same_dsp_clock_ce": same_enable, "parameters": params})
    passed = (bool(scopes) and len(results) == 2 * len(scopes) and len(macs) == len(scopes)
              and all({r["consumer"] for r in results if r["cell"].rsplit("/", 1)[0] == scope}
                      == {24, 28} for scope in scopes)
              and all(r["same_prescaled_q"] for r in results)
              and all(r["hard_output_only"] and r["same_dsp_clock_ce"] for r in macs))
    return {"netlist_sha256": hashlib.sha256(netlist.read_bytes()).hexdigest(),
            "passed": passed, "cells": results, "macs": macs,
            "scope": "Selected mapped operand provenance/clock/DSP CE; source replay separately checks CE0 Q hold."}


def qualify_netlist(netlist, output):
    receipt = audit(netlist)
    assert receipt["passed"], "baseline mapped boundary audit failed"
    source = netlist.read_text()
    z = next(cell for cell in receipt["cells"] if cell["consumer"] == 24)
    scope = z["cell"].rsplit("/", 1)[0]
    y = next(cell for cell in receipt["cells"] if cell["consumer"] == 28
             and cell["cell"].rsplit("/", 1)[0] == scope)
    # Wrong registered data must fail too; checking merely 'some FF Q' is weak.
    cell = re.search(r"(?m)^MULT9X9\s+" + re.escape(z["cell"])
                     + r"\s+\(.*?\n\);", source, re.S)[0]
    mac = next(cell["cell"] for cell in receipt["macs"]
               if cell["cell"].rsplit("/", 1)[0] == scope)
    parameter = re.search(r"(?m)^defparam\s+" + re.escape(mac)
                          + r"\s+\.OUT_REG=1'b1;", source)[0]
    mutants = [
        ("wrong-low-owner", source.replace(cell, cell.replace(
            f".B({z['B']})", f".B({y['B']})", 1), 1)),
        ("wrong-high-owner", source.replace(cell, cell.replace(
            f".A({z['A']})", f".A({y['A']})", 1), 1)),
        ("mac-unregistered", source.replace(parameter, parameter.replace("1'b1", "1'b0"), 1)),
    ]
    negatives = []
    for name, changed in mutants:
        assert changed != source
        path = output / (name + ".vo")
        path.write_text(changed)
        assert not audit(path)["passed"], f"mapped negative survived: {name}"
        negatives.append({"name": name, "caught": True})
    return {"passed": True, "mapped": receipt, "negatives": negatives}


BENCH = r"""`timescale 1ns/1ps
module tb;
GSR GSR(.GSRI(1'b1));reg clk=0;always #5 clk=~clk;
reg reset=1;integer cycles=0,sent=0,received=0;

wire ce=cycles%13<11;wire ready,valid;wire[8:0]g,h;wire[31:0]id;
`ifdef GPU_V2_GOWIN_DSP
reg[85:0]paused_q;
`ifdef RB09_PHYSICAL
always@(posedge clk)if(!reset&&ce&&sent>0)begin
 if(dut.dsp24.B!=={2'b00,dut.unit12_p0[6:0]} ||
    dut.dsp28.B!=={2'b00,dut.unit11_p0[6:0]})$fatal(1,"prescaled Q age at DSP B");
end
`endif
always@(posedge clk)if(!reset&&!ce)begin
 paused_q={dut.unit11_p0,dut.unit12_p0,dut.unitRB09_MAC_tail};
 #1;if({dut.unit11_p0,dut.unit12_p0,dut.unitRB09_MAC_tail}!==paused_q)$fatal(1,"CE0 physical Q changed");
end
`endif
reg context_valid=0;wire context_ready;
always@(posedge clk)if(!reset&&ce&&context_valid&&context_ready)begin #1;context_valid=0;end
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
gpu_v2_lighting dut(.clk(clk),.reset(reset),.ce(ce),RB09_CONTEXT_PORTS.context_epoch(16'd0),
.context_mode(mode),.context_code(code),.light_x(lx),.light_y(ly),.light_z(lz),
.ambient(ia),.directional(di),.ray_x(rx),.ray_y(ry),.ray_k(rk),
.in_valid(offer),.in_ready(ready),.in_id(sent),.in_row0(r0),.in_row1(r1),.in_row2(r2),
.out_valid(valid),.out_ready(1'b1),.out_id(id),.out_epoch(),.out_g(g),.out_h(h));
always@(posedge clk)begin
if(cycles>1000)$fatal(1,"bounded replay");
if(!reset&&ce)begin
 if(valid)begin
  if(id!==received||g!==eg||h!==eh)$fatal(1,"id=%0d g/h=%0d/%0d expected=%0d/%0d",id,g,h,eg,eh);
  $display("TOKEN cycle=%0d id=%0d g=%0d h=%0d",cycles,id,g,h);
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
lx=12534;ly=8667;lz=6018;rx=-8192;ry=-4915;rk=8192;ia=64;di=192;code=12;mode=3;context_valid=1;
end
endmodule
"""


def replay(rtl, output, portable=False):
    output.mkdir(parents=True, exist_ok=True)
    source = rtl.read_text()
    (output / "core.v").write_text(
        module(source, "gpu_v2_lighting") + "\n" + module(source, "lighting_norm_q13")
    )
    mac = re.search(r"MULTADDALU18X18 #\(.*?\) dsp(\d+)\(", module(source, "gpu_v2_lighting"), re.S)[1]
    light = module(source, "gpu_v2_lighting")
    context = ".context_valid(context_valid),.context_ready(context_ready)," if "context_valid" in light.split(");", 1)[0] else ""
    physical = "(* syn_preserve=1, syn_keep=1 *) reg [15:0] unit12_p0;" in light
    (output / "tb.v").write_text(BENCH.replace("RB09_MAC", mac).replace("RB09_CONTEXT_PORTS", context))
    library = Path(os.environ["GOWIN_HOME"]) / "IDE/simlib/gw2a/prim_sim.v"
    commands = [
        [os.environ["IVERILOG_EXE"], "-g2012", *([] if portable else ["-DGPU_V2_GOWIN_DSP"]), *(["-DRB09_PHYSICAL"] if physical else []), "-s", "tb", "-o",
         str(output / "sim.vvp"), str(output / "core.v"), str(output / "tb.v"), str(library)],
        [os.environ["VVP_EXE"], str(output / "sim.vvp")],
    ]
    for label, command in zip(["compile", "replay"], commands):
        result = subprocess.run(command, capture_output=True, text=True, timeout=60)
        (output / (label + ".log")).write_text(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError(f"{label} failed; see {output / (label + '.log')}")
    trace = (output / "replay.log").read_text().splitlines()
    tokens = [line for line in trace if line.startswith("TOKEN ")]
    assert len(tokens) == 12
    return {"passed": True, "rtl_sha256": hashlib.sha256(rtl.read_bytes()).hexdigest(),
            "portable": portable, "tokens": tokens, "cycle_limit": 1000}


def qualify(rtl, output):
    """Keep the short native/fabric replay and three physical-age negatives together."""
    native = replay(rtl, output / "native")
    portable = replay(rtl, output / "portable", portable=True)
    assert native["tokens"] == portable["tokens"], "native/fabric enabled-edge mismatch"
    source = rtl.read_text()
    light = module(source, "gpu_v2_lighting")
    old = ".PIPE1_REG(1'b0),.OUT_REG(1'b1)"
    assert old in light
    mutants = [("mac-unregistered", light.replace(old, ".PIPE1_REG(1'b0),.OUT_REG(1'b0)", 1))]
    for coordinate, producer in [("z", 12), ("y", 11)]:
        current_d = re.search(rf"unit{producer}_p0 <= ([^;]+);", light)[1]
        old = f".B({{2'b00,square_{coordinate}_tail_q}})"
        assert old in light
        mutants.append((coordinate + "-current-d", light.replace(
            old, f".B({{2'b00,{current_d}[6:0]}})", 1)))
    negatives = []
    for name, changed in mutants:
        directory = output / name
        directory.mkdir(parents=True)
        mutant = directory / "dut.v"
        mutant.write_text(source.replace(light, changed))
        try:
            replay(mutant, directory / "replay")
        except RuntimeError:
            # An elaboration failure is not evidence that the data check caught it.
            log = directory / "replay/replay.log"
            assert log.exists(), f"{name} failed before simulation"
            text = log.read_text()
            assert "FATAL" in text and any(t in text for t in ["id=", "CE0", "bounded replay", "prescaled Q age"])
            negatives.append({"name": name, "caught": True})
        else:
            raise AssertionError(f"physical-age negative survived: {name}")
    return {"passed": True, "native": native, "portable": portable, "negatives": negatives}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--netlist", type=Path)
    parser.add_argument("--rtl", type=Path)
    parser.add_argument("--qualify", action="store_true", help="Check replay or mapped-boundary negative controls in the same process")
    parser.add_argument("--portable", action="store_true", help="Replay the fabric DSP fallback")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if bool(args.netlist) == bool(args.rtl):
        parser.error("select exactly one of --netlist and --rtl")
    args.output.mkdir(parents=True, exist_ok=True)
    if args.netlist:
        receipt = qualify_netlist(args.netlist, args.output) if args.qualify else audit(args.netlist)
    elif args.qualify:
        receipt = qualify(args.rtl, args.output)
    else:
        receipt = replay(args.rtl, args.output, args.portable)
    (args.output / "receipt.json").write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt))
    raise SystemExit(0 if receipt["passed"] else 1)
