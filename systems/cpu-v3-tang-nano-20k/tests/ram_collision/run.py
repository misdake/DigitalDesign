"""Bounded FPU/cache/Display collision-Q checks against freshly exported RTL."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

sys.dont_write_bytecode = True
import fpu_program
import models

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def replace_once(text, old, new):
    assert text.count(old) == 1, f"changed test hook: {old}"
    return text.replace(old, new)


def cache_bench(peer):
    # Reuse the existing legal cache peer; only CPU response stalls are added.
    text = peer[:peer.index("initial begin")]
    text = text[:text.index("task copy_line;")]
    text = replace_once(text, "reg [15:0] memory [0:262143];",
                        "reg [15:0] memory [0:262143];reg[15:0]oracle[0:262143];")
    text = replace_once(text, "cpu_write_data=value; cpu_request_valid=1;",
                        "cpu_write_data=value; cpu_request_valid=1; if(wr) oracle[address]=value;")
    text = replace_once(text, "cpu_response_ready=1; @(posedge clk); #1; cpu_response_ready=0;", """repeat(3)begin @(negedge clk);
 if(!cpu_response_valid || cpu_error || (!wr && cpu_read_data!==expected))
  $fatal(1,"AUDIT_LIVE_HELD_RESPONSE addr=%h",address);
 end
 cpu_response_ready=1; @(posedge clk); #1; cpu_response_ready=0;""")
    return "`timescale 1ns/1ps\n" + text + (HERE / "dcache_checks.vh").read_text()


def bounded(command, directory, logfile, limit):
    try:
        result = subprocess.run(command, cwd=directory, capture_output=True,
                                text=True, timeout=limit)
        logfile.write_text(result.stdout + result.stderr)
        return result
    except subprocess.TimeoutExpired as error:
        def decode(value):
            return value.decode(errors="replace") if isinstance(value, bytes) else value or ""
        logfile.write_text(decode(error.stdout) + decode(error.stderr) + "\nWALL_TIMEOUT\n")
        raise RuntimeError(f"process exceeded {limit}s: {logfile}") from error


def executable(variable, fallback):
    found = os.environ.get(variable) or shutil.which(fallback)
    if not found:
        raise RuntimeError(f"set {variable} or put {fallback} on PATH")
    return str(Path(found).resolve())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path,
                        default=ROOT / "target/ram-collision-consumption/rtl")
    parser.add_argument("--output", type=Path,
                        default=ROOT / "target/ram-collision-consumption/run")
    parser.add_argument("--case", choices=["all", "fpu", "dcache", "display"], default="all")
    args = parser.parse_args()
    args.input, args.output = args.input.resolve(), args.output.resolve()
    if not args.output.is_relative_to((ROOT / "target").resolve()):
        parser.error("output must be inside this checkout's target directory")
    args.output.mkdir(parents=True, exist_ok=True)
    iv, vv = executable("IVERILOG_EXE", "iverilog"), executable("VVP_EXE", "vvp")
    gowin = os.environ.get("GOWIN_HOME")
    if not gowin:
        parser.error("set GOWIN_HOME to the Gowin installation containing IDE/simlib/gw2a/prim_sim.v")
    primitive = Path(gowin).resolve() / "IDE/simlib/gw2a/prim_sim.v"
    assert primitive.is_file(), primitive
    reference = json.loads((HERE / "reference.json").read_text())
    export_identity = json.loads((args.input / "export.json").read_text())
    exports = [args.input / path for path in export_identity["files"]]
    assert all(path.is_file() for path in exports), "incomplete RTL export"
    assert exports, "export production inputs with the ignored Rust export test first"

    def exported(name):
        found = [path for path in exports if path.name == name]
        assert len(found) == 1, (name, found)
        return found[0]

    peer = ROOT / "ip/cpu-v3/src/hardware/cache/cpu_v3_data_cache_tb.v"
    body = ROOT / "ip/cpu-v3/src/hardware/cache/cpu_v3_data_cache_vendor_data_body.vh"
    changes = {
        exported("cpu_v3_fpu_exec_rf.v"): lambda source: models.rf(source, reference),
        exported("display_line_buffer.v"): lambda source: models.display(source, reference),
        exported("imagesh6fe6fc706d18f126.v"):
            lambda source: models.cache(source, body.read_text(), reference),
    }
    sources = []
    for path in exports:
        dest = args.output / "rtl" / path.relative_to(args.input)
        dest.parent.mkdir(parents=True, exist_ok=True)
        source = path.read_text()
        dest.write_text(changes[path](source) if path in changes else source)
        sources.append(dest)
    manifest = {
        "reference": reference,
        "export": export_identity,
        "exported_sha256": {str(path.relative_to(args.input)): digest(path) for path in exports},
        "fixture_sha256": {path.name: digest(path) for path in HERE.iterdir() if path.is_file()},
        "tracked_peer_sha256": {str(path.relative_to(ROOT)): digest(path) for path in [peer, body]},
        "tools": {"iverilog": iv, "vvp": vv, "primitive": str(primitive),
                  "primitive_sha256": digest(primitive)},
        "cases": {},
    }
    (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    definitions = ["-DCPU_V3_CACHE_METADATA_VENDOR"]
    for case in ["fpu", "dcache", "display"]:
        if args.case not in ("all", case):
            continue
        directory = args.output / case
        directory.mkdir(exist_ok=True)
        if case == "fpu":
            bench = fpu_program.render((HERE / "fpu_tb.v").read_text(), directory)
        elif case == "dcache":
            bench = cache_bench(peer.read_text())
        else:
            bench = (HERE / "display_tb.v").read_text()
        tb = directory / "tb.v"
        tb.write_text(bench)
        for variant, plusargs, fault in [
            ("baseline", [], None), ("poison", ["+POISON"], None),
            ("detector", ["+POISON", "+FORCE_LIVE"], {
                "fpu": "AUDIT_LIVE_OPERAND_A", "dcache": "AUDIT_LIVE_LOAD",
                "display": "AUDIT_LIVE_PIXEL",
            }[case]),
        ]:
            output = directory / variant
            output.mkdir(exist_ok=True)
            compile_command = [iv, "-g2012", *definitions, "-s", "tb", "-o",
                               str(output / "sim.vvp"), str(tb),
                               *map(str, sources), str(primitive)]
            run_command = [vv, str(output / "sim.vvp"), *plusargs]
            cap = 90 if case == "display" else 60
            commands = {"compile": compile_command, "run": run_command,
                        "compile_wall_cap_seconds": 60, "run_wall_cap_seconds": cap,
                        "source_sha256": {str(path): digest(path)
                                          for path in [tb, *sources, primitive]}}
            (output / "commands.json").write_text(json.dumps(commands, indent=2) + "\n")
            print(f"START {case}/{variant}", flush=True)
            started = time.monotonic()
            compiled = bounded(compile_command, output, output / "compile.log", 60)
            assert compiled.returncode == 0, output / "compile.log"
            result = bounded(run_command, output, output / "run.log", cap)
            log = result.stdout + result.stderr
            counters = {} if fault else {key: int(value) for key, value in
                        re.findall(r"(\w+)=(\d+)(?=\s|$)", log)}
            record = dict(returncode=result.returncode, detector=fault,
                          seconds=round(time.monotonic() - started, 3), counters=counters)
            passed = (result.returncode != 0 and fault in log) if fault else (
                result.returncode == 0 and "DIGITAL_DESIGN_PASS" in log and "FATAL" not in log)
            record["passed"] = passed
            manifest["cases"][f"{case}/{variant}"] = record
            (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
            assert passed, output / "run.log"
            print(f"PASS {case}/{variant}: {counters or fault}", flush=True)
    print("RAM collision consumption checks passed; no board or physical timing qualification.")


if __name__ == "__main__":
    main()
