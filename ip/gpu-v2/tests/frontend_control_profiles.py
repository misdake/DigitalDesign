"""Bounded real-DUT frontend profile, sticky-stop and native-owner regressions."""
from pathlib import Path
import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
import os
import re
import subprocess
import sys

HERE = Path(__file__).resolve().parent
OWNERS = {"shared_meshlet", "shared_frontend", "rp_prepare"}
IVERILOG = os.environ.get("IVERILOG_EXE", "iverilog")
VVP = os.environ.get("VVP_EXE", "vvp")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def call(cmd, directory, label, timeout=90):
    result = subprocess.run(cmd, cwd=directory, capture_output=True, timeout=timeout)
    log = result.stdout + result.stderr
    (directory / (label + ".log")).write_bytes(log)
    return result.returncode, log.decode(errors="replace")


def tokens(source):
    source = re.sub(r"/\*.*?\*/|//[^\n]*", "", source, flags=re.S)
    return re.findall(r'"(?:\\.|[^"\\])*"|[A-Za-z_$][\w$]*|\d+\x27[sS]?[bBdDhHoO][0-9a-fA-F_xXzZ?]+|\d+|\S', source)


def preprocess(exports, output, baseline=None):
    rows = []
    for export in sorted(exports.glob("*.v")):
        for synthesis in [False, True]:
            sets = []
            for side, root in [("candidate", exports), ("baseline", baseline)]:
                if root is None:
                    continue
                folder = output / "preprocessed" / side / ("synthesis" if synthesis else "simulation")
                folder.mkdir(parents=True, exist_ok=True)
                source = root / export.name
                # A concatenated caller must not inherit the private semantic profile.
                probe = source.read_text() + "\nmodule frontend_profile_scope_probe;\n`ifdef GPU_V2_TRUSTED_FRONTEND\nlocalparam LEAK=1;\n`else\nlocalparam LEAK=0;\n`endif\nendmodule\n"
                src, dst = folder / (export.stem + "-probe.v"), folder / export.name
                src.write_text(probe)
                cmd = [IVERILOG, "-g2012", "-E", *(["-DSYNTHESIS"] if synthesis else []), "-o", str(dst), str(src)]
                code, log = call(cmd, folder, export.stem + "-compile", 30)
                assert code == 0, log[-3000:]
                modules = [(name, tokens(body)) for body, name in re.findall(r"(^module\s+(\w+)\b.*?endmodule)", dst.read_text(), re.M | re.S)]
                assert modules[-1][1] == tokens("module frontend_profile_scope_probe;localparam LEAK=0;endmodule"), export
                sets.append(modules)
            if len(sets) == 2:
                assert sets[0] == sets[1], (export, synthesis, "ordered preprocessed circuit changed")
            rows.append(dict(export=export.name, synthesis=synthesis, modules=len(sets[0])-1,
                             baseline_equal=len(sets) == 2, export_sha256=sha(export)))
    return rows


def pingpong(export, output, native):
    spec = importlib.util.spec_from_file_location("frontend_profile_pingpong", HERE / "vliw_meshlet_pingpong.py")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    # Observe the existing independent-oracle testbench, without editing its DUT.
    observer = """
 always @(negedge clk)if(!reset)$display("TRACE V %0d %b %b %b %b %b %b %h %b %h %h %b %h %h %b %h %h",
 cycle,ce,ready,busy,done,drained,dma_ready,error,emit_valid,emit_addr,emit_data,
 packet_valid,descriptor,published_vertices,word_valid,word_index,word_data);
"""
    helper.TB = helper.TB.replace("\nendmodule", observer + "\nendmodule", 1)
    old = sys.argv
    sys.argv = ["pingpong", "--export", str(export), "--output", str(output), "--full-queue", "--stream-reads"]
    if native:
        sys.argv.append("--native")
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            helper.main()
    finally:
        sys.argv = old
    text = (output / "run.log").read_text()
    trace = "\n".join(line for line in text.splitlines() if line.startswith("TRACE "))
    assert trace, "real handshake trace absent"
    return hashlib.sha256(trace.encode()).hexdigest()


def index_input(pingpong_output):
    dma = {}
    for line in (pingpong_output / "task0/dma.txt").read_text().splitlines():
        addr, value = line.split()
        dma[int(addr)] = int(value, 16)
    old = (dma[256] >> 24) & 127
    dma[127] = dma[old]
    # Four triangles require a second group after real indices at the final row.
    dma[256] = (dma[256] & ~((127 << 24) | (127 << 8))) | (127 << 24) | (4 << 8)
    return "".join(f"{addr} {value:016x}\n" for addr, value in sorted(dma.items()))


def control_case(export, output, case, trusted, synthesis, dma, mutation=None):
    output.mkdir(parents=True)
    (output / "dma.txt").write_text(dma)
    source = export.read_text()
    if mutation:
        old = "&&!(|{error[3],error[0]})"
        # Scope the mutation to the real prepare ready or CE expression.
        if mutation == "ready":
            target = "assign in_ready=!reset&&core_active&&input_phase" + old + ";"
            replacement = "assign in_ready=!reset&&core_active&&input_phase;"
        else:
            target = "rp_prepare_core core(clk,reset,ce" + old + ",core_start,"
            replacement = "rp_prepare_core core(clk,reset,ce,core_start,"
        assert source.count(target) == 1, mutation
        source = source.replace(target, replacement)
    dut = output / "dut.v"
    dut.write_text(source)
    unit = output / "sim.vvp"
    cmd = [IVERILOG, "-g2012", "-s", "tb", "-Ptb.CASE=" + str(case),
           "-Ptb.TRUSTED=" + str(int(trusted)), *(["-DSYNTHESIS"] if synthesis else []),
           "-o", str(unit), str(dut), str(HERE / "frontend_control_profiles_tb.v")]
    code, log = call(cmd, output, "compile")
    assert code == 0, log[-3000:]
    code, log = call([VVP, str(unit)], output, "run", 20)
    trace = [line for line in log.splitlines() if line.startswith("TRACE ")]
    if mutation:
        expected = "prepare " + ("ready" if mutation == "ready" else "core CE") + " ignored sticky error"
        assert code != 0 and expected in log, (mutation, log[-2000:])
    elif trusted and not synthesis:
        assert code != 0 and ("GPU contract:" in log or "packed import layout" in log), log[-2000:]
        assert "PASS " not in log, "invalid trusted input escaped its original monitor"
    else:
        assert code == 0 and "PASS " in log, log[-3000:]
    # File/line diagnostics change with native source layout; compare the label,
    # last real sampled edge and all prior DUT observations instead.
    fatal = [line.split(": ", 1)[-1] for line in log.splitlines() if line.startswith("FATAL:")]
    if fatal:
        fatal = [re.sub(r"^.*?:\d+: ", "", line) for line in fatal]
    return dict(case=case, trusted=trusted, synthesis=synthesis, mutation=mutation,
                trace_sha256=hashlib.sha256("\n".join(trace).encode()).hexdigest(),
                fatal=fatal, returncode=code, sampled_edges=len(trace))


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--exports", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--baseline", type=Path)
    args = parser.parse_args()
    exports, output = args.exports.resolve(), args.output.resolve()
    baseline = args.baseline.resolve() if args.baseline else None
    output.mkdir(parents=True, exist_ok=False)
    rows = preprocess(exports, output, baseline)
    valid = []
    for native in [False, True]:
        mode = "native" if native else "portable"
        traces = []
        for side, root in [("candidate", exports), ("baseline", baseline)]:
            if root is None:
                continue
            folder = output / ("valid-" + side + "-" + mode)
            traces.append(pingpong(root / ("frontend-" + mode + ".v"), folder, native))
        assert len(set(traces)) == 1, (mode, "cycle trace changed")
        valid.append(dict(native=native, trace_sha256=traces[0], baseline_equal=baseline is not None))
    dma = index_input(output / "valid-candidate-portable")
    cases = []
    # Primitive implementations are checked by the valid native/portable replay.
    # The control rejection matrix uses portable hardware to keep this gate small.
    for profile in ["frontend-portable", "frontend-command-ports-portable", "command-portable"]:
        trusted = profile == "command-portable"
        for case in [1, 2, 3, 4, 10, 11, 20, 21]:
            for synthesis in ([False, True] if trusted and case in [10, 11] else [not trusted]):
                pair = []
                for side, root in [("candidate", exports), ("baseline", baseline)]:
                    if root is None:
                        continue
                    folder = output / f"{side}-{profile}-{case}-synth{int(synthesis)}"
                    pair.append(control_case(root / (profile + ".v"), folder, case, trusted, synthesis, dma))
                assert all(row == pair[0] for row in pair), (profile, case, pair)
                cases.append(dict(profile=profile, **pair[0], baseline_equal=baseline is not None))
    mutations = []
    for mutation in ["ready", "ce"]:
        mutations.append(control_case(exports / "frontend-portable.v", output / ("mutation-" + mutation),
                                      10, False, True, dma, mutation))
    receipt = dict(status="pass", preprocessed=rows, valid=valid, cases=cases, mutations=mutations,
                   maximum_dut_cycles=12000, compile_timeout_seconds=90, run_timeout_seconds=20,
                   scope="Owner circuits and real CE/hold/backpressure/error policies; no full scene or physical qualification")
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(dict(status="pass", exports=len(rows)//2, valid=len(valid), cases=len(cases), mutations=len(mutations))))


if __name__ == "__main__":
    main()
