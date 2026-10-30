"""Bounded phase invariant replay of the final production compute and Raster RTL."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def module(source, name):
    match = re.search(r"\bmodule\s+" + name + r"\b.*?\bendmodule\b", source, re.S)
    assert match, name
    return match.group()


def ports(source):
    header = source[:source.index(");")]
    header = re.sub(r"\(\*.*?\*\)", "", header, flags=re.S)
    header = header[header.index("input"):]
    direction, width = None, ""
    for part in header.split(","):
        part = part.strip()
        decl = re.match(r"(input|output)\b\s*(?:(?:wire|reg|signed)\s*)*(\[[^]]+\])?\s*(\w+)$", part)
        if decl:
            direction, width, name = decl.groups()
            width = width or ""
        else:
            assert direction and re.fullmatch(r"\w+", part), part
            name = part
        yield direction, width, name


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--export", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = args.export.read_text(encoding="utf-8")
    compute = module(source, "rc_compute")
    raster = module(source, "rc_raster")
    frontend = module(source, "gpu_v2_vliw_frontend")
    interpolator = module(source, "gpu_v2_vliw_interpolator")
    for text, instance in ((frontend, "rc_raster raster"),
                           (frontend, "gpu_v2_vliw_interpolator interpolator"),
                           (interpolator, "rc_compute compute")):
        binding = text[text.index(instance):].split(");", 1)[0]
        for pin in ("clk", "reset", "ce", "phase"):
            assert f".{pin}({pin})" in binding, (instance, pin)
    declarations, instances = [], []
    for name, text in (("rc_compute", compute), ("rc_raster", raster)):
        links = []
        for direction, width, port in ports(text):
            if direction == "input":
                value = port if port in ("clk", "reset", "ce") else (
                    "producer_phase" if port == "phase" else "'0")
            elif name == "rc_compute" and port == "phase":
                value = "producer_phase"
            else:
                value = f"{name}_{port}"
                declarations.append(f"wire {width} {value};")
            links.append(f".{port}({value})")
        instances.append(f"{name} {name}_dut(" + ",".join(links) + ");")
    tb = r"""`timescale 1ns/1ps
module tb;
reg clk=0,reset=1,run_ce=0,abort=0;
wire ce=run_ce&&!abort;
wire[1:0]producer_phase;
reg[1:0]before_phase,expected;
integer i,checks=0,stalls=0,aborts=0,restarts=0;
reg[3:0]paused_phases=0,active_phases=0;
GSR GSR(.GSRI(1'b1));
@DECL@
@INST@
initial begin
 for(i=0;i<256;i=i+1)begin
  reset=i<3||(i>=112&&i<115)||i==132||i==149||i==166||i==183;
  abort=i>=96&&i<112;
  run_ce=i>=3&&!(i>=112&&i<115)&&(i%6!=2);
  #2;before_phase=producer_phase;
  expected=reset?2'd0:ce?(before_phase+2'd1):before_phase;
  if(!reset)begin
   if(ce)active_phases[before_phase]=1;
   else begin paused_phases[before_phase]=1;stalls=stalls+1;end
   if(abort)aborts=aborts+1;
  end else restarts=restarts+1;
  clk=1;#1;
  if(producer_phase!==expected||rc_raster_dut.raster_phase!==expected)
   $fatal(1,"phase next-state cycle=%0d expected=%0d producer=%0d replica=%0d",i,expected,producer_phase,rc_raster_dut.raster_phase);
  checks=checks+1;#2;clk=0;#1;
 end
 if(active_phases!==4'b1111||paused_phases!==4'b1111||aborts!=16||checks!=256)
  $fatal(1,"phase coverage active=%b paused=%b aborts=%0d checks=%0d",active_phases,paused_phases,aborts,checks);
 $display("PASS phase invariant checks=%0d stalls=%0d aborts=%0d reset_edges=%0d",checks,stalls,aborts,restarts);
 $finish;
end
initial begin #2000;$fatal(1,"phase replay wall-clock watchdog");end
endmodule
""".replace("@DECL@", "\n".join(declarations)).replace("@INST@", "\n".join(instances))
    (out / "tb.v").write_text(tb)
    primitive = Path(os.environ["GOWIN_HOME"]) / "IDE/simlib/gw2a/prim_sim.v"
    cases = []
    for variant in ("candidate", "delayed-q-negative"):
        rtl = source if variant == "candidate" else source.replace(
            "else if(ce)raster_phase<=phase+1'b1;", "else if(ce)raster_phase<=phase;")
        assert variant == "candidate" or rtl != source
        dut = out / f"{variant}.v"
        dut.write_text(rtl, encoding="utf-8")
        for stage, command in (
            ("compile", [os.environ["IVERILOG_EXE"], "-g2012", "-s", "tb", "-o",
                         str(out / "sim.vvp"), str(dut), str(out / "tb.v"), str(primitive)]),
            ("run", [os.environ["VVP_EXE"], str(out / "sim.vvp")]),
        ):
            result = subprocess.run(command, cwd=out, capture_output=True, timeout=45)
            log = result.stdout.decode(errors="replace") + result.stderr.decode(errors="replace")
            (out / f"{variant}-{stage}.log").write_text(log)
            if stage == "run" and variant != "candidate":
                assert result.returncode != 0 and "phase next-state" in log, log[-1500:]
            else:
                assert result.returncode == 0, log[-1500:]
        cases.append(dict(variant=variant, result=log.strip()))
    receipt = dict(status="pass", final_export_sha256=hashlib.sha256(args.export.read_bytes()).hexdigest(),
                   shared_controls="frontend and interpolator clock/reset/CE/phase bindings checked",
                   scope="256 actual-module phase edges, all four paused/active phases, abort frontend freeze and reset/restart; active pixels tested separately",
                   cases=cases)
    (out / "receipt.json").write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
