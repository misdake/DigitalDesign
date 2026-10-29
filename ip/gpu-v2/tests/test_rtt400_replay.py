"""Replay actual RCC submissions; independently check every fullscreen pixel.

Axis modes initialize a column-layout address code before reset and isolate the
nearest consumer. Their pair uniquely identifies all 96000 source coordinates.
Chain mode retains real GPU producer writes between submissions. Neither mode
converts or rearranges pixels between GPU jobs.
"""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import re
import struct
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
HELPERS = ROOT / "ip/gpu-v2/examples/vliw_integration"


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def pixel_word(x, y):
    return ((x // 16) * 16 + y // 16) * 256 + (y % 16) * 16 + x % 16


def memory_text(parts):
    text = []
    for address, data in parts:
        assert address % 8 == 0 and len(data) % 8 == 0
        text.append(f"@{address // 8:x}\n")
        text.extend(f"{word[0]:016x}\n" for word in struct.iter_unpack("<Q", data))
    return "".join(text)


TB = r'''
`timescale 1ns/1ps
module tb;
@PORTS@
GSR GSR(.GSRI(1'b1));always #5 clk=~clk;
gpu_v2_vliw_command dut(.*);
reg[63:0]memory[0:1048575];
reg ro_active=0,rd_active=0,wr_active=0;
integer cycle=0,rb=0,ri=0,rn=0,fb=0,fi=0,wb=0,wi=0,j,x,y,index,current_job=-1,events;
always@(negedge clk)begin
 ce=cycle%7!=2;
 ro_request_ready=!ro_active&&cycle%5!=1;
 ro_response_valid=ro_active&&cycle%3!=1;
 ro_response_last=ri==rn-1;ro_response_error=0;
 ro_data=ro_active?memory[rb+ri]:0;
 fb_r_request_ready=!rd_active&&!wr_active;
 fb_r_response_valid=rd_active&&cycle%3!=2;
 fb_r_response_last=fi==15;fb_r_response_error=0;
 fb_r_data=rd_active?memory[fb+fi]:0;
 fb_w_request_ready=!wr_active&&!rd_active;
 fb_w_data_ready=wr_active&&cycle%4!=2;
 fb_w_response_valid=wr_active&&wi==15&&fb_w_data_ready;
 fb_w_response_last=1;fb_w_response_error=0;
end
always@(posedge clk)begin
 cycle=cycle+1;
 if(cycle>32000000)$fatal(1,"RTT replay cycle bound");
 if(!reset)begin
  if(fault||device_read_data[1])$fatal(1,"RTT replay DUT fault at cycle %0d",cycle);
  if(cycle%100000==0)$display("RTT replay progress cycle=%0d busy=%0d ro=%0d rd=%0d wr=%0d",cycle,busy,ro_active,rd_active,wr_active);
  @OBSERVE@
  if(ro_request_valid&&ro_request_ready)begin
   ro_active=1;rb=ro_address>>2;ri=0;rn=ro_count==3?16:ro_count==1?8:4;
  end else if(ro_response_valid)begin if(ro_response_last)ro_active=0;else ri=ri+1;end
  if(fb_r_request_valid&&fb_r_request_ready)begin rd_active=1;fb=fb_r_address>>2;fi=0;end
  else if(fb_r_response_valid)begin if(fi==15)rd_active=0;else fi=fi+1;end
  if(fb_w_request_valid&&fb_w_request_ready)begin wr_active=1;wb=fb_w_address>>2;wi=0;end
  else if(wr_active&&fb_w_data_ready)begin memory[wb+wi]=fb_w_data;wi=wi+1;if(wi==16)wr_active=0;end
 end
end
task tick;begin @(negedge clk);#1;end endtask
task finish_job;
 begin
  while(busy)tick;
  if(device_read_data[1]||fault||ro_active||rd_active||wr_active)$fatal(1,"RTT replay terminal");
 end
endtask
initial begin
 events=$fopen("sample-events.txt","w");
 for(j=0;j<1048576;j=j+1)memory[j]=64'ha55aa55aa55aa55a;
 $readmemh("initial.hex",memory);
 @AXIS_INIT@
 reset=1;output_ready=1;tick;tick;reset=0;
 @JOBS@
 $fclose(events);
 $display("PASS RTT_REPLAY mode=@MODE@ jobs=@COUNT@ cycles=%0d",cycle);$finish;
end
endmodule
'''


def read_plane(path):
    words = []
    for line in path.read_text().splitlines():
        line = line.split("//")[0].strip()
        if line:
            words.extend(struct.unpack("<4H", int(line, 16).to_bytes(8, "little")))
    assert len(words) == 131072
    return words


def sampled_rgb(plane, raw_u, raw_v, linear):
    def axis(raw, size):
        signed = raw if raw < 131072 else raw - 262144
        uv = max(0, min(65535, signed))
        fine = max(0, min((size-1)*256, uv*size//256-(128 if linear else 0)))
        return divmod(fine, 256)
    x, fx = axis(raw_u, 400)
    y, fy = axis(raw_v, 240)
    bottom = 511*fy//256
    top = 511-bottom
    right, br = top*fx//256, bottom*fx//256
    weights = [top-right, right, bottom-br, br] if linear else [511, 0, 0, 0]
    rgb = [0, 0, 0]
    for tap, weight in enumerate(weights):
        word = plane[pixel_word(min(399, x+tap%2), min(239, y+tap//2))]
        r, g, b = word>>11, (word>>5)&63, word&31
        for c, v in enumerate([(r<<3)|(r>>2), (g<<2)|(g>>4), (b<<3)|(b>>2)]):
            rgb[c] += (weight*v+256)//512
    return sum(min(255, c) << (8*i) for i, c in enumerate(rgb))


def modulated_word(word, tint):
    r, g, b = word >> 11, (word >> 5) & 63, word & 31
    expanded = [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]
    channels = [(value * shade + 127) // 255 for value, shade in zip(expanded, tint)]
    return sum(((value * maximum + 127) // 255) << shift
               for value, maximum, shift in zip(channels, [31, 63, 31], [11, 5, 0]))


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--export", type=Path, required=True)
    parser.add_argument("--inputs", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--mode", choices=("axis-x", "axis-y", "chain"), required=True)
    parser.add_argument("--backend", choices=("native", "portable"), required=True)
    parser.add_argument("--tint", type=int, nargs=3, default=[255, 255, 255])
    args = parser.parse_args()
    assert all(0 <= shade <= 255 for shade in args.tint)
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = args.export.resolve()
    inputs = args.inputs.resolve()
    recording = json.loads(inputs.read_text())
    assert recording["profile"] == "rtt400x240-column16"
    assert recording["surface"] == "column-major-16x16-pad16"
    catalog = json.loads((inputs.parent / recording["assets"]).read_text())
    parts = [(r["address"], (inputs.parent / r["file"]).read_bytes()) for r in catalog["resources"]]
    (out / "initial.hex").write_text(memory_text(parts))
    selected = [1] if args.mode.startswith("axis-") else list(range(len(recording["submissions"])))
    consumer = recording["submissions"][1]
    command_bytes = (inputs.parent / consumer["command_file"]).read_bytes()
    commands = [value[0] for value in struct.iter_unpack("<I", command_bytes)]
    color_clears = [value & 65535 for value in commands if value >> 28 == 8 and not value & 65536]
    assert len(color_clears) == 1
    color_clear = color_clears[0]
    jobs = []
    for number in selected:
        job = recording["submissions"][number]
        parts = [(job["command_base_bytes"], (inputs.parent / job["command_file"]).read_bytes()),
                 (job["object_base_bytes"], (inputs.parent / job["object_file"]).read_bytes())]
        (out / f"job{number}.hex").write_text(memory_text(parts))
        jobs.append(f'current_job={number};$readmemh("job{number}.hex",memory);submit_commands(32\'h{job["command_base_bytes"]:x},{job["valid_instructions"]*4});finish_job;')
        for plane in ("color", "depth"):
            base = job[f"{plane}_base_bytes"] // 8
            jobs.append(f'$writememh("job{number}-{plane}.hex",memory,{base},{base+32767});')
    wrapper = load("rtt_replay_wrapper", HELPERS.parent / "postquad_ii4/wrapper.py")
    _, ports = wrapper.wrapper(re.sub(r"\b(input|output)(?=\[)", r"\1 ", source.read_text()))
    declarations = []
    for port in ports["ports"]:
        width = f'[{port["bits"]-1}:0]' if port["bits"] > 1 else ""
        declarations.append(("reg " if port["direction"] == "input" else "wire ") + width + " " + port["name"] + ("=0;" if port["direction"] == "input" else ";"))
    axis = ""
    if args.mode.startswith("axis-"):
        base = recording["submissions"][0]["color_base_bytes"] // 8
        value = "x" if args.mode == "axis-x" else "y"
        axis = f'''for(y=0;y<240;y=y+1)for(x=0;x<400;x=x+1)begin
 index=((x/16)*16+y/16)*256+(y%16)*16+x%16;
 memory[{base}+index/4][(index%4)*16+:16]={value};end'''
    sample = load("rtt_replay_hierarchy", HELPERS / "texture_layout_replay.py").hierarchy(source.read_text(), "gpu_v2_vliw_sampling")
    observe = ""
    if args.mode == "chain":
        observe = f'''if({sample}.texture_fb400240)begin
 if({sample}.sample_fire&&{sample}.sample_descriptor[5+{sample}.sample_row])
  $fdisplay(events,"S %0d %0d %0d %h %h %0d",current_job,{sample}.sample_descriptor[3:0],{sample}.sample_row,{sample}.sample_data[17:0],{sample}.sample_data[35:18],{sample}.texture_filter);
 if({sample}.color_result)$fdisplay(events,"C %0d %0d %h",current_job,{sample}.color_key,{sample}.color_rgb);
end'''
    tb = TB.replace("@PORTS@", "\n".join(declarations)).replace("@AXIS_INIT@", axis).replace("@OBSERVE@", observe).replace("@JOBS@", "\n".join(jobs)).replace("@MODE@", args.mode).replace("@COUNT@", str(len(selected)))
    tb = load("rtt_replay_device", HELPERS / "device_bus.py").bench(tb)
    (out / "tb.v").write_text(tb)
    commands = [([os.environ["IVERILOG_EXE"], "-g2012", "-s", "tb", "-o", "sim.vvp", source, out / "tb.v", Path(os.environ["GOWIN_HOME"]) / "IDE/simlib/gw2a/prim_sim.v"], "compile", 180),
                ([os.environ["VVP_EXE"], "sim.vvp"], "run", 900)]
    for command, label, timeout in commands:
        with (out / (label + ".log")).open("wb") as log:
            result = subprocess.run(list(map(str, command)), cwd=out, stdout=log, stderr=subprocess.STDOUT, timeout=timeout)
        assert result.returncode == 0, (out / (label + ".log")).read_text(errors="replace")[-3000:]
    verified = 0
    if args.mode.startswith("axis-"):
        plane = read_plane(out / "job1-color.hex")
        errors = []
        for y in range(240):
            for x in range(400):
                actual = plane[pixel_word(x, y)]
                expected = modulated_word(x if args.mode == "axis-x" else y, args.tint)
                if actual != expected and len(errors) < 12:
                    errors.append(dict(x=x, y=y, actual=actual, expected=expected))
                verified += int(actual == expected)
        assert verified == 96000, dict(verified=verified, first_errors=errors)
        for y in range(240, 256):
            for x in range(400):
                assert plane[pixel_word(x, y)] == color_clear, ("macro padding", x, y)
        assert all(v == 0xa55a for v in plane[102400:]), "allocation tail was written"
    samples = 0
    if args.mode == "chain":
        planes = {n: read_plane(out / f"job{n}-color.hex") for n in selected}
        pending = {}
        for line in (out / "sample-events.txt").read_text().splitlines():
            fields = line.split()
            job = int(fields[1])
            if fields[0] == "S":
                key = (job, int(fields[2])*4+int(fields[3]))
                assert key not in pending, ("owner reuse", key)
                pending[key] = sampled_rgb(planes[job-1], int(fields[4], 16), int(fields[5], 16), int(fields[6]) == 1)
            else:
                key = (job, int(fields[2]))
                assert pending.pop(key) == int(fields[3], 16), ("sample result", key, line)
                samples += 1
        assert not pending and samples > 0
        for n, plane in planes.items():
            assert all(v == 0xa55a for v in plane[102400:]), ("allocation tail", n)
            depth = read_plane(out / f"job{n}-depth.hex")
            assert all(v == 0xa55a for v in depth[102400:]), ("depth allocation tail", n)
        if len(selected) == 2 and samples == 96000:
            assert all(planes[1][pixel_word(x,y)] == modulated_word(planes[0][pixel_word(x,y)], args.tint) for y in range(240) for x in range(400)), "nearest producer/consumer identity"
            verified = 96000
    receipt = dict(status="pass", mode=args.mode, backend=args.backend, tint=args.tint, color_clear_rgb565=color_clear, checked_pixels=verified, independently_checked_samples=samples, export_sha256=hashlib.sha256(source.read_bytes()).hexdigest(), inputs_sha256=hashlib.sha256(inputs.read_bytes()).hexdigest(),
                   boundary="Actual RCC-generated commands through the ordinary GPU pipeline with the explicitly selected backend and bounded stalled memory peers. Axis modes independently isolate addressing using pre-reset source patterns. Chain mode retains real producer writes and checks sampling at actual raster UV against an independent integer oracle. No physical MC/STA/board claim.")
    (out / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
