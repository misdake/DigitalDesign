"""Matched registered-boundary fits for the production CP CLEAR command.

The CP and full command-profile fits use the union of both exported interfaces.
The final source-chain bit is observed explicitly, so an unused trailing input
in the baseline cannot remove probe FFs and masquerade as DUT area overhead.
No scene simulation, source mutation, board access or experiment RTL is used.
"""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
HELPERS = ROOT / "ip/gpu-v2/examples/postquad_ii4"
CONTROLS = {"clk", "ce", "reset"}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def normalize(source):
    return re.sub(r"\b(input|output)(?=\[)", r"\1 ", source.replace("\r\n", "\n"))


def select_scope(source, scope):
    if scope == "whole":
        return normalize(source)
    if scope == "packet":
        # The sole default parameter is unused in this selected two-edge leaf.
        # Remove only that declaration to expose its exact ports to the common
        # fixed-boundary probe; neither variant's datapath/control is changed.
        marker = "module packet_leaf_unique #(parameter SHORT_ALIGNMENT=1)("
        start = source.index(marker)
        end = source.index("endmodule", start) + len("endmodule")
        return normalize(source[start:end].replace(marker, "module packet_leaf_unique("))
    # Move the CP first but retain the original dependency closure. Unreachable
    # complete-GPU modules are not instantiated by the CP probe and are trimmed.
    selected = {"cp": "gpu_command_processor", "dma": "gpu_command_dma",
                "light": "gpu_v2_lighting"}[scope]
    start = re.search(r"^module\s+" + selected + r"\s*\(", source, re.M)
    assert start, "The real exported control module is missing: " + selected
    end = source.index("endmodule", start.end()) + len("endmodule")
    return normalize(source[start.start():end] + "\n" + source[:start.start()] + source[end:])


def boundary(source, parser):
    module = re.search(r"^module\s+(\w+)\s*\(", source, re.M)
    assert module, "Expected an exported top module first"
    _, info = parser.wrapper(source)
    return module[1], info["ports"]


def union_ports(base, candidate):
    result = []
    known = {}
    for port in base + candidate:
        name = port["name"]
        if name in known:
            assert port == known[name], ("Incompatible physical port", known[name], port)
        else:
            known[name] = port
            result.append(port)
    return result


def probe(source, actual, union):
    module = re.search(r"^module\s+(\w+)\s*\(", source, re.M)[1]
    groups = {d: [p for p in union if p["direction"] == d and p["name"] not in CONTROLS]
              for d in ("input", "output")}
    widths = {d: sum(p["bits"] for p in group) for d, group in groups.items()}
    iw, ow = widths["input"], widths["output"]
    assert iw > 0 and ow > 0
    present = {p["name"] for p in actual}
    assert CONTROLS.issubset(present)
    text = f"""module timing_top(input clk,ce,reset,serial_in,output[5:0]leds);
wire[{iw-1}:0]src;wire[{ow-1}:0]sink;
genvar i;generate for(i=0;i<{iw};i=i+1)begin:source_boundary
if(i==0)DFFE ff(.Q(src[i]),.D(serial_in),.CLK(clk),.CE(ce));
else DFFE ff(.Q(src[i]),.D(src[i-1]),.CLK(clk),.CE(ce));
end endgenerate
"""
    connections = [f".{p}({p})" for p in ("clk", "ce", "reset")]
    offsets = {}
    for direction, bus in (("input", "src"), ("output", "sink")):
        offset = 0
        for p in groups[direction]:
            signal = f"{bus}[{offset} +: {p['bits']}]"
            offsets[p["name"]] = dict(bus=bus, offset=offset, bits=p["bits"])
            if p["name"] in present:
                connections.append(f".{p['name']}({signal})")
            elif direction == "output":
                text += f"assign {signal}={p['bits']}'d0;\n"
            offset += p["bits"]
    text += module + " dut(" + ",".join(connections) + ");\n"
    for observer in range(6):
        taps = [f"sink[{bit}]" for bit in range(observer, ow, 6)]
        if observer == 0:
            taps.append(f"src[{iw-1}]")
        expr = "^{" + ",".join(taps) + "}" if taps else "1'b0"
        text += f"DFFE observe{observer}(.Q(leds[{observer}]),.D({expr}),.CLK(clk),.CE(ce));\n"
    text += "endmodule\n"
    return text, dict(input_bits=iw, output_bits=ow, ports=union,
                      actual_ports=actual, mapping=offsets,
                      registered_wrapper_ff=iw+6, source_tail_observed=True,
                      scope="Union boundary; full input chain retained by explicit tail observer")


def fit_one(label, scope, source, actual, union, out, placement, mhz, dry_run, reuse_base=None):
    helper = load("clear_fit_" + scope + "_" + label, HELPERS / "fit_helper.py")
    helper.OUT = out / scope / label / "fits"
    helper.SOURCE = out / scope / label / "source"
    helper.GW = os.environ.get("GW_SH_EXE") or str(Path(os.environ["GOWIN_HOME"]) / "IDE/bin/gw_sh.exe")
    wrapped, info = probe(source, actual, union)
    helper.wrapper = lambda _: (wrapped, info)
    dest = helper.SOURCE / "selected"
    dest.mkdir(parents=True, exist_ok=False)
    (dest / "native.v").write_text(source)
    (dest / "probe.v").write_text(wrapped)
    receipt = dict(label=label, scope=scope, export_source_sha256=hashlib.sha256(source.encode()).hexdigest(),
                   script_sha256=sha(Path(__file__)), boundary=info, dry_run=dry_run)
    if reuse_base is not None:
        assert label == "base" and not dry_run
        old = json.loads(reuse_base.read_text())
        old_work = reuse_base.parent / f"fits/selected-native-p{placement}-m{mhz}"
        assert old["scope"] == scope
        assert old["boundary"] == info, "Reused fit has a different physical probe boundary"
        assert old["placement"] == placement and old["mhz"] == mhz and old["strict_pass"]
        assert old["export_source_sha256"] == receipt["export_source_sha256"]
        assert sha(old_work / "dut.v") == old["snapshot_sha256"] == sha(dest / "native.v")
        assert sha(old_work / "top.v") == old["wrapper_sha256"] == sha(dest / "probe.v")
        assert sha(old_work / "impl/pnr/leaf.rpt.txt") == old["report_sha256"]
        assert sha(old_work / "impl/pnr/leaf.tr") == old["timing_sha256"]
        run_tcl = (old_work / "run.tcl").read_text()
        assert "GW2AR-LV18QN88C8/I7" in run_tcl
        assert f"set_option -place_option {placement}" in run_tcl
        assert f"-period {1000/mhz:.6f}" in (old_work / "timing.sdc").read_text()
        receipt.update(old)
        receipt["label"] = label
        receipt["reused_original_label"] = old["label"]
        receipt["reused_receipt"] = str(reuse_base.resolve())
        receipt["reused_receipt_sha256"] = sha(reuse_base)
        receipt["reuse_validator_sha256"] = sha(Path(__file__))
        receipt["reuse_verified"] = "Exact exported source, complete probe, device/placement/clock, report and timing identities"
    elif not dry_run:
        result = helper.run(("selected", "native", placement, mhz))
        receipt.update(result)
        report = helper.OUT / f"selected-native-p{placement}-m{mhz}/impl/pnr/leaf.rpt.txt"
        if report.exists():
            report_text = report.read_text(errors="replace")
            receipt["primitives"] = {k: int(v) for k, v in re.findall(r"--(MULT\w+|ALU54D)\s*\|\s*(\d+)", report_text)}
            for key, pattern in [("logic_ff", r"--Logic Register as FF\s*\|\s*(\d+)"),
                                 ("bsram", r"^\s*BSRAM\s*\|\s*([\d.]+)"),
                                 ("dsp", r"^\s*DSP\s*\|\s*([\d.]+)")]:
                match = re.search(pattern, report_text, re.M)
                receipt[key] = float(match[1]) if match else None
        receipt["strict_pass"] = (receipt.get("returncode") == 0 and receipt.get("logic") is not None
                                  and receipt.get("setup_violations") == 0 and receipt.get("hold_violations") == 0)
    (out / scope / label / "receipt.json").write_text(json.dumps(receipt, indent=2))
    keys = ("label", "scope", "logic", "logic_ff", "bsram", "dsp", "fmax", "setup_violations", "hold_violations", "strict_pass")
    print(json.dumps({k: receipt.get(k) for k in keys}), flush=True)
    return receipt


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--base", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--scope", choices=("cp", "dma", "light", "packet", "whole", "both"), default="both")
    parser.add_argument("--placement", choices=(0, 1), type=int, default=0)
    parser.add_argument("--mhz", type=int, default=54)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--reuse-base", type=Path, help="Use a matching strict passed scope receipt as baseline after exact identity checks")
    args = parser.parse_args()
    assert args.reuse_base is None or (args.scope != "both" and not args.dry_run), "Reuse requires a single scope"
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    sources = {label: path.resolve() for label, path in (("base", args.base), ("candidate", args.candidate))}
    summary = dict(inputs={label: dict(path=str(path), sha256=sha(path)) for label, path in sources.items()},
                   head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                   placement=args.placement, mhz=args.mhz, fits=[],
                   scope="Registered CP and/or standalone complete GPU command profile; no CPU/Display/MC or board claim")
    pp = load("clear_boundary_parser", HELPERS / "wrapper.py")
    for scope in (("cp", "whole") if args.scope == "both" else (args.scope,)):
        selected = {label: select_scope(path.read_text(), scope) for label, path in sources.items()}
        ports = {label: boundary(source, pp)[1] for label, source in selected.items()}
        union = union_ports(ports["base"], ports["candidate"])
        for label in ("base", "candidate"):
            summary["fits"].append(fit_one(label, scope, selected[label], ports[label], union, out,
                                            args.placement, args.mhz, args.dry_run,
                                            args.reuse_base if label == "base" else None))
            (out / "receipt.json").write_text(json.dumps(summary, indent=2))
    summary["strict_pass"] = None if args.dry_run else all(r["strict_pass"] for r in summary["fits"])
    (out / "receipt.json").write_text(json.dumps(summary, indent=2))
    if not args.dry_run and not summary["strict_pass"]:
        raise SystemExit("A fit or strict timing check failed; see receipt.json and Gowin logs")


if __name__ == "__main__":
    main()
