"""Collect MC synthesis hierarchy, full-project PnR area/timing, and image hashes.

The synthesis hierarchy is not a per-module post-route allocation. Never label
its LUT+ALU totals as measured board utilization or whole-project PnR Logic.
"""
import argparse
import hashlib
import json
import re
import xml.etree.ElementTree as ET
from pathlib import Path


def fnv_update(value, data):
    for byte in data:
        value = ((value ^ byte) * 0x100000001b3) & 0xffffffffffffffff
    return value


def verify_build_identity(root):
    # Match the authoritative Rust build manifest, including constraints,
    # wrapper, project and tool options. Reject a stale source/image pairing.
    manifest = dict(line.split("=", 1) for line in (root / "gowin-build.manifest").read_text().splitlines())
    value = 0xcbf29ce484222325
    for raw in (root / ".digital-design-generated").read_text().splitlines():
        path = raw.replace(chr(92), "/")
        for data in [path.encode("utf-8"), (root / path).read_bytes()]:
            value = fnv_update(value, len(data).to_bytes(8, "little"))
            value = fnv_update(value, data)
    if f"{value:016x}" != manifest["source_fingerprint"]:
        raise ValueError("generated sources changed; rebuild before measuring/programming")
    image = (root / manifest["bitstream_path"]).read_bytes()
    if len(image) != int(manifest["bitstream_bytes"]) or f"{fnv_update(0xcbf29ce484222325, image):016x}" != manifest["bitstream_fingerprint"]:
        raise ValueError("bitstream changed after qualified build")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project", type=Path)
    args = parser.parse_args()
    root = args.project
    build = verify_build_identity(root)
    reports = list((root / "impl/gwsynthesis").glob("*_syn_rsc.xml"))
    if len(reports) != 1:
        raise ValueError("expected exactly one top synthesis hierarchy")
    name = reports[0].name.removesuffix("_syn_rsc.xml")
    hierarchy = ET.parse(reports[0]).getroot()
    modules = {}

    def visit(element, path):
        name = element.attrib["name"]
        path = f"{path}/{name}" if path else name
        modules[path] = {k: int(element.attrib.get(k, 0)) for k in ["Lut", "Alu", "Register"]}
        modules[path]["logic_lut_plus_alu"] = modules[path]["Lut"] + modules[path]["Alu"]
        for child in element:
            visit(child, path)
    visit(hierarchy, "")
    mc = {k: v for k, v in modules.items() if "/u_sdram_bridge" in k or k.endswith("/arbiter") or k.endswith("/adapter")}
    sums = {name: sum(v[name] for v in mc.values()) for name in ["Lut", "Alu", "Register", "logic_lut_plus_alu"]}
    report = (root / f"impl/pnr/{name}.rpt.txt").read_text()
    pnrarea = {}
    for key in ["Logic", "Register", "CLS", "PLL"]:
        match = re.search(rf"^\s*{key}\s+\|\s*(\d+)/", report, re.M)
        pnrarea[key] = int(match[1]) if match else None
    timing = (root / f"impl/pnr/{name}_tr_content.html").read_text()
    fmax = {name: float(mhz) for name, mhz in re.findall(r"<td>(controller_clk|cpu_clk)</td>\s*<td>[\d.]+\(MHz\)</td>\s*<td>([\d.]+)\(MHz\)</td>", timing)}
    slacks = {}
    for kind in ["Setup", "Hold"]:
        table = timing.split(f'<a name="{kind}_Slack_Table">', 1)[1].split("</table>", 1)[0]
        values = [float(v) for v in re.findall(r"<tr[^>]*>\s*<td>\d+</td>\s*<td>(-?[\d.]+)</td>", table)]
        if not values or min(values) < 0:
            raise ValueError(f"missing/negative {kind} slack: {values[:5]}")
        slacks[kind.lower()] = min(values)
    fs = root / f"impl/pnr/{name}.fs"
    if not fs.exists() or fmax.get("controller_clk", 0) < 108 or fmax.get("cpu_clk", 0) < 54:
        raise ValueError("missing bitstream or clock qualification")
    sources = sorted(root.glob("src/generated/**/*.v")) + sorted(root.glob("src/generated/*.sdc")) + sorted(root.glob("src/generated/*.cst"))
    hashes = {str(p.relative_to(root)).replace("\\", "/"): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
    result = dict(schema=1, board_qualification="not run", build_source_fingerprint=build["source_fingerprint"], mc_synthesis_exclusive_modules=mc,
                  mc_synthesis_sum=sums, full_project_pnr=pnrarea, fmax_mhz=fmax, worst_slack_ns=slacks,
                  bitstream_sha256=hashlib.sha256(fs.read_bytes()).hexdigest(), source_sha256=hashes)
    (root / "measurement-manifest.json").write_text(json.dumps(result, indent=2)+"\n", encoding="utf-8")
    print(json.dumps({k: v for k, v in result.items() if k not in ["source_sha256", "mc_synthesis_exclusive_modules"]}, indent=2))


if __name__ == "__main__":
    main()
