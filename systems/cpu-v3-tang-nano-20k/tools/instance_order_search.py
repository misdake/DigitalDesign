"""Bounded instance-order experiments on an immutable exported Gowin project.

This is an output-block transform, not a general Verilog parser. Only the twelve
named CpuV3System instances are movable. All other bytes and source-file ordering
are frozen. Icarus elaboration checks named nets, widths and nexus aliases before
Gowin runs. Vendor-library diagnostics are recorded separately, never hidden.
"""
import argparse
import ast
import collections
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import random
import re
import shutil
import subprocess
import time
import xml.etree.ElementTree as ET

NAMES = "u_instruction_fetch_queue u_instruction_cache u_ds2 u_sysctl u_boot_loader u_flash u_data_cache u_core u_gpu u_memory_arbiter u_shared_sdram_port u_display".split()
TOP = "src/generated/examples/cpu_v3_system/cpu_v3_system.v"
OUTPUT = "cpu_v3_system"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def save(path, value):
    temp = path.with_suffix(path.suffix + ".tmp")
    temp.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    temp.replace(path)


def inputs(project):
    return {p.relative_to(project).as_posix(): digest(p.read_bytes())
            for p in sorted((project / "src").rglob("*")) if p.is_file()}


def job_environment(project):
    directory = project / "tmp"
    directory.mkdir(exist_ok=True)
    return dict(os.environ, TEMP=str(directory), TMP=str(directory))


def lexical_view(text):
    return re.sub(r'//[^\r\n]*|/\*.*?\*/|"(?:\\.|[^"\\])*"',
                    lambda m: "".join(c if c in "\r\n" else " " for c in m[0]),
                    text, flags=re.S)


def blocks(text):
    # Mask without moving offsets. Balanced parentheses include named parameters,
    # nested expressions and comments; an unrecognised/duplicate block fails shut.
    masked = lexical_view(text)

    def skip(i):
        while i < len(masked) and masked[i].isspace():
            i += 1
        return i

    def parens(i):
        if masked[i] != "(":
            raise ValueError("Expected parentheses")
        depth = 0
        for j in range(i, len(masked)):
            depth += (masked[j] == "(") - (masked[j] == ")")
            if depth == 0:
                return j + 1
        raise ValueError("Unclosed instance")

    found = []
    for match in re.finditer(r"(?m)^[ \t]*(?:__[A-Z0-9_]+__|[A-Za-z_]\w*)\b", masked):
        i = skip(match.end())
        if i < len(masked) and masked[i] == "#":
            i = skip(parens(skip(i + 1)))
        name = re.match(r"[A-Za-z_]\w*", masked[i:])
        if not name or name[0] not in NAMES:
            continue
        i = skip(i + len(name[0]))
        if masked[i] != "(":
            raise ValueError("Unsupported instance declaration")
        end = skip(parens(i))
        if masked[end] != ";":
            raise ValueError("Expected complete single-instance declaration")
        end += 1
        if text[end:end + 2] == "\r\n":
            end += 2
        elif text[end:end + 1] == "\n":
            end += 1
        found.append((match.start(), end, name[0], text[match.start():end]))
    if len(found) != len(NAMES) or {b[2] for b in found} != set(NAMES):
        raise ValueError("Expected exactly the twelve known instance declarations")
    return found


def residue(text, spans):
    last, out = 0, []
    for start, end, _, _ in spans:
        out.append(text[last:start])
        last = end
    return "".join(out) + text[last:]


def permute(text, order):
    spans = blocks(text)
    bodies = {b[2]: b[3] for b in spans}
    if len(order) != len(NAMES) or set(order) != set(NAMES):
        raise ValueError("Order must be a bijection of the twelve instance names")
    rest = residue(text, spans)
    # The generated file also contains fixed helper modules after CpuV3System.
    # All movable blocks must belong to its first module; retain later modules.
    original_end = re.search(r"(?m)^endmodule\b", lexical_view(text))
    if not original_end or any(s[1] > original_end.start() for s in spans):
        raise ValueError("Movable instances must all belong to the first module")
    end = re.search(r"(?m)^endmodule\b", lexical_view(rest)).start()
    return rest[:end] + "".join(bodies[n] for n in order) + rest[end:]


def structure(before, after):
    a, b = blocks(before), blocks(after)
    if {s[2]: s[3] for s in a} != {s[2]: s[3] for s in b}:
        raise ValueError("Instance type/parameters/connections/body changed")
    if residue(before, a) != residue(after, b):
        raise ValueError("Non-instance bytes or their relative order changed")
    return {"old_order": [s[2] for s in a], "order": [s[2] for s in b],
            "non_instance_sha256": digest(residue(before, a).encode()),
            "instance_sha256": {s[2]: digest(s[3].encode()) for s in b}}


def net_manifest(text):
    """Canonical declared-net geometry, hierarchy and named-net nexus aliases.

    Ignore Icarus-generated local expression names and process addresses. Keep
    every named declared signal and unpacked pin. Expression bodies are protected
    independently by exact source/body equality, not inferred from this manifest.
    """
    signals, nexuses, hierarchy = {}, collections.defaultdict(list), []
    current = None
    for line in text.splitlines():
        m = re.match(r"^(\S+) module <([^>]+)> instance (\d+) children", line)
        if m:
            hierarchy.append(m.groups())
        m = re.match(r"    (wire|reg)(?: /\*implicit\*/)?:(\s+\S+).*scope=(\S+).*vector_width=(\d+) pin_count=(\d+)", line)
        if m:
            kind, name, scope, width, pins = m.groups()
            name = name.strip()
            current = None
            if "(local)" not in line and not name.startswith("_ivl_"):
                current = scope + "." + name
                signal_type = re.search(r"netvector_t:(.*?) \(eref=", line)
                direction = re.search(r"\b(input|output|inout)\b", line)
                signals[current] = [kind, int(width), int(pins), signal_type[1] if signal_type else "", direction[1] if direction else "internal"]
        elif current:
            p = re.match(r"\s+\[(\d+)\]: (0x[0-9a-fA-F]+) ", line)
            if p:
                nexuses[p[2]].append(current + "#" + p[1])
            else:
                current = None
    if len(hierarchy) < 12 or not signals:
        raise ValueError("Unrecognised Icarus elaboration dump")
    for name in NAMES:
        if not any(h[0] == "tang_nano_20k_ds2_top.u_logic." + name for h in hierarchy):
            raise ValueError("Missing elaborated instance " + name)
    return {"hierarchy": sorted(list(h) for h in hierarchy), "signals": dict(sorted(signals.items())),
            "aliases": sorted(sorted(set(v)) for v in nexuses.values())}


def elaborate(project, vendor, expected=None):
    # Strict net declarations apply only to the selected system module. Restore
    # the default afterwards; do not rewrite vendor or other module sources.
    top = (project / TOP).read_bytes().decode()
    check = project / "checked-system.v"
    check.write_bytes(("`default_nettype none\n" + top + "\n`default_nettype wire\n").encode())
    sources = [p for p in sorted((project / "src").rglob("*.v")) if p != project / TOP]
    cmd = [os.environ.get("IVERILOG_EXE", "iverilog"), "-g2012", "-Wall",
           "-s", "tang_nano_20k_ds2_top", "-s", "GSR", "-N", str(project / "elaborated.net"),
           "-o", str(project / "elaborated.vvp"), *map(str, sources), str(check), str(vendor)]
    start = time.monotonic()
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=90, env=job_environment(project))
    log = result.stdout + result.stderr
    (project / "elaboration.log").write_text(log, encoding="utf-8")
    # The immutable SDK model has its own known diagnostics. They are retained
    # in the log/receipt; zero project implicit-net or width diagnostics allowed.
    project_diagnostics = [s for s in log.splitlines() if str(vendor).lower() not in s.lower()]
    bad = [s for s in project_diagnostics if re.search(r"implicit definition|expects .* got|dangling input|error:", s, re.I)]
    if result.returncode or bad:
        raise ValueError("Elaboration/port-width check failed: " + "\n".join(bad[:8]))
    manifest = net_manifest((project / "elaborated.net").read_text())
    save(project / "elaborated-connections.json", manifest)
    if expected is not None and manifest != expected:
        raise ValueError("Elaborated named nets/widths/hierarchy/connections differ")
    info = {"command": cmd, "seconds": time.monotonic() - start,
            "connection_sha256": digest(json.dumps(manifest, sort_keys=True).encode()),
            "declared_signals": len(manifest["signals"]), "modules": len(manifest["hierarchy"]),
            "project_width_or_implicit_diagnostics": bad,
            "vendor_diagnostics": [s for s in log.splitlines() if str(vendor).lower() in s.lower()],
            "vendor_sha256": digest(vendor.read_bytes())}
    save(project / "elaboration.json", info)
    return manifest


def metrics(project):
    out = {}
    synth = project / "impl/gwsynthesis" / (OUTPUT + "_syn_rsc.xml")
    if synth.exists():
        root = ET.parse(synth).getroot()
        out["synthesis"] = root.attrib
        out["synthesis_instances"] = [s.attrib["name"] for s in root.find(".//*[@name='u_logic']")]
    timing = project / "impl/pnr" / (OUTPUT + ".tr")
    if timing.exists():
        text = timing.read_text()
        for kind in ("Setup", "Hold"):
            out[kind.lower() + "_violations"] = int(re.search(r"<Numbers of " + kind + r" Violated Endpoints>:(\d+)", text)[1])
            section = text.split("3.1." + ("1 Setup" if kind == "Setup" else "2 Hold") + " Paths Table")[-1].split("3.1." + ("2 Hold" if kind == "Setup" else "3 Recovery"))[0]
            rows = re.findall(r"^\s+\d+\s+(-?\d+\.\d+)\s+(.+)$", section, re.M)
            out[kind.lower() + "_wns_ns"] = min(float(r[0]) for r in rows) if rows else None
            out[kind.lower() + "_paths"] = rows
            negative = {}
            for slack, row in rows:
                if float(slack) < 0:
                    endpoint = row.split()[1]
                    negative[endpoint] = min(float(slack), negative.get(endpoint, 0))
            out[kind.lower() + "_reported_negative_slack_sum_ns"] = sum(negative.values())
            out[kind.lower() + "_negative_endpoints_complete"] = len(negative) == out[kind.lower() + "_violations"]
        summary = text.split("2.4 Total Negative Slack Summary")[-1].split("3. Timing Details")[0]
        for kind in ("setup", "hold"):
            out[kind + "_tns_ns"] = sum(float(v) for v in re.findall(r"^\s+\S+\s+" + kind + r"\s+(-?\d+\.\d+)\s+\d+\s*$", summary, re.M))
        out["tns_scope"] = "Gowin per-clock summary; may exclude cross-clock paths. Negative endpoint sums are complete only when the corresponding completeness flag is true."
    report = project / "impl/pnr" / (OUTPUT + ".rpt.txt")
    if report.exists():
        text = report.read_text()
        out["area"] = {name: int(re.search(r"^\s+" + name + r"\s+\|\s+(\d+)/", text, re.M)[1]) for name in ("Logic", "Register", "BSRAM", "DSP")}
        match = re.search(r"\((\d+) LUT, (\d+) ALU", text)
        out["area"].update(LUT=int(match[1]), ALU=int(match[2]))
        match = re.search(r"Total Time and Memory Usage:.*Elapsed time = (\d+)h (\d+)m ([\d.]+)s, Peak memory usage = (\d+)MB", text)
        if match:
            out["pnr_seconds"] = int(match[1]) * 3600 + int(match[2]) * 60 + float(match[3])
            out["pnr_peak_memory_mb"] = int(match[4])
    logpath = project / "run.log"
    log = logpath.read_text(errors="replace") if logpath.exists() else ""
    out["unrouted_nets"] = max(map(int, re.findall(r"total (\d+) unrouted nets", log)), default=0)
    return out


def score(result):
    # Invalid elaboration or unexpected trimming is never optimisation feedback.
    if not result.get("valid"):
        return None
    if result.get("qualified"):
        return [0, result["area"]["Logic"], -result["setup_wns_ns"]]
    if result.get("tool_exit") == 0 and "setup_violations" in result and not result["unrouted_nets"]:
        return [1, max(0, -result["setup_wns_ns"]) + max(0, -(result["hold_wns_ns"] or 0)),
                -result["setup_reported_negative_slack_sum_ns"] - result["hold_reported_negative_slack_sum_ns"], result["area"]["Logic"]]
    return [2, result.get("unrouted_nets", 10**9) or 10**9]


def swept(project):
    log = project / "impl/gwsynthesis" / (OUTPUT + ".log")
    if not log.exists():
        return []
    return sorted(re.findall(r'The module "([^"]+)" instantiated to "([^"]+)" is swept', log.read_text(errors="replace")))


def synthesis_diagnostics(project):
    log = project / "impl/gwsynthesis" / (OUTPUT + ".log")
    if not log.exists():
        return []
    # Existing expression-result truncations are not port connection warnings.
    # Keep their exact signatures/counts from the immutable baseline; any new
    # diagnostic is invalid. Other 31 source files are also byte-protected.
    return sorted(s.replace(str(project.resolve()), "@PROJECT@")
                  for s in log.read_text(errors="replace").splitlines()
                  if re.search(r"implicit|truncat|padd|width mismatch", s, re.I))


def classify(result, project, expected_swept, expected_diagnostics=()):
    missing = set(NAMES) - set(result.get("synthesis_instances", []))
    observed = synthesis_diagnostics(project)
    unexpected = sorted((collections.Counter(observed) - collections.Counter(expected_diagnostics)).elements())
    extra_swept = sorted((collections.Counter(swept(project)) - collections.Counter(expected_swept)).elements())
    result["invalid_reasons"] = {"missing_synthesis_instances": sorted(missing),
                                 "synthesis_diagnostics": unexpected, "extra_swept_modules": extra_swept}
    result["unchanged_baseline_synthesis_diagnostic_count"] = len(observed) - len(unexpected)
    result["valid"] = not missing and not unexpected and not extra_swept
    result["qualified"] = bool(result["valid"] and result["tool_exit"] == 0 and result.get("setup_violations") == 0
                               and result.get("hold_violations") == 0 and result.get("unrouted_nets") == 0)
    result["score"] = score(result)


def run_job(base, job, order, vendor, expected, deadline, expected_swept=(), expected_diagnostics=()):
    job.mkdir(parents=True, exist_ok=False)
    result = {"order": order, "valid": False, "qualified": False, "directory": str(job)}
    start = time.time()
    try:
        shutil.copytree(base / "src", job / "src")
        shutil.copyfile(base / "build.tcl", job / "build.tcl")
        before = (base / TOP).read_bytes().decode()
        after = permute(before, order)
        result["structure"] = structure(before, after)
        (job / TOP).write_bytes(after.encode())
        result["inputs"] = inputs(job)
        elaborate(job, vendor, expected)
        script = (job / "build.tcl").read_text()
        if script.count("run all") != 1:
            raise ValueError("Unsupported build script")
        (job / "run-recorded.tcl").write_text(script.replace("run all", "saveto -all_options [file join $here effective-options.tcl]\nrun all"))
        command = [str(Path(os.environ["GOWIN_HOME"]) / "IDE/bin/gw_sh.exe"), "run-recorded.tcl"]
        result["command"] = command
        with (job / "run.log").open("w") as log:
            process = subprocess.Popen(command, cwd=job, stdout=log, stderr=subprocess.STDOUT, env=job_environment(job))
            result["pid"] = process.pid
            save(job / "running.json", result)
            try:
                result["tool_exit"] = process.wait(timeout=max(1, min(1200, deadline - time.time())))
            except subprocess.TimeoutExpired:
                # Kill only this owned subprocess tree, not any other Gowin job.
                if os.name == "nt":
                    subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True)
                else:
                    process.kill()
                process.wait()
                result["tool_exit"] = "timeout"
        result.update(metrics(job))
        classify(result, job, expected_swept, expected_diagnostics)
        if inputs(job) != result["inputs"]:
            raise ValueError("Tool modified an immutable input")
        netlist = job / "impl/gwsynthesis" / (OUTPUT + ".vg")
        if netlist.exists():
            result["seconds_until_netlist_emission"] = netlist.stat().st_mtime - start
        result["artifact_sha256"] = {p.relative_to(job).as_posix(): digest(p.read_bytes())
                                     for p in job.rglob("*") if p.is_file() and (p.suffix in (".tcl", ".do", ".vg", ".tr", ".xml", ".fs", ".prj", ".gprj", ".posp") or p.name.endswith(".rpt.txt"))}
    except Exception as error:
        result["error"] = str(error)
        result["valid"] = False
        result["qualified"] = False
    result["elapsed_seconds"] = time.time() - start
    result["score"] = score(result)
    save(job / "result.json", result)
    return result


def portfolio(results):
    ranked = sorted((r for r in results if r.get("score") is not None and r["score"][0] <= 1), key=lambda r: r["score"])
    if not ranked:
        return []
    margin = lambda r: min(r["setup_wns_ns"], r["hold_wns_ns"] if r["hold_wns_ns"] is not None else r["setup_wns_ns"])
    selected = [ranked[0]]
    # Keep both extremes of the area/margin frontier, a near-feasible start,
    # then other frontier and order-diverse candidates. These are parents only:
    # failed timing never becomes delivery qualification.
    frontier, best_margin = [], -float("inf")
    for r in sorted(ranked, key=lambda r: (r["area"]["Logic"], -margin(r))):
        if margin(r) > best_margin:
            frontier.append(r)
            best_margin = margin(r)
    near = next((r for r in ranked if not r["qualified"]), None)
    anchors = selected + [frontier[0], max(frontier, key=margin)] + ([near] if near else [])
    far = max(ranked, key=lambda r: min(sum(a != b for a, b in zip(r["order"], p["order"])) for p in anchors))
    choices = anchors[1:] + [far] + frontier + ranked
    for r in choices:
        if all(r["order"] != p["order"] for p in selected):
            if r in frontier or r is near or all(sum(a != b for a, b in zip(r["order"], p["order"])) >= 3 for p in selected):
                selected.append(r)
        if len(selected) == 6:
            break
    return selected


def propose(rng, results, seen, stale):
    initial = [NAMES, ["u_gpu"] + [n for n in NAMES if n != "u_gpu"],
               [n for n in NAMES if n != "u_gpu"] + ["u_gpu"], list(reversed(NAMES))]
    for order in initial:
        if tuple(order) not in seen:
            return order
    parents = portfolio(results)
    for attempt in range(100):
        # Rank weights favour performance while every retained parent stays
        # eligible. A small uniform exploration probability keeps other basins.
        if parents:
            parent = (rng.choice(parents) if rng.random() < 0.15 else
                      rng.choices(parents, weights=range(len(parents), 0, -1), k=1)[0])["order"]
        else:
            parent = NAMES
        order = list(parent)
        edits = 3 if stale >= 4 and attempt % 4 == 0 else 1
        for _ in range(edits):
            a, b = rng.sample(range(len(order)), 2)
            if rng.randrange(2):
                order[a], order[b] = order[b], order[a]
            else:
                order.insert(b, order.pop(a))
        if tuple(order) not in seen:
            return order
    raise RuntimeError("Finite neighbour budget exhausted; stop instead of looping")


def tuple_tree(value):
    return tuple(tuple_tree(v) for v in value) if isinstance(value, list) else value


def cache_key(context, order):
    return digest(json.dumps([context, order], sort_keys=True).encode())


def active_pid(pid):
    # Never use os.kill(pid, 0) on Windows: its semantics are different there.
    if os.name == "nt":
        import ctypes
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.restype = ctypes.c_void_p
        kernel.GetExitCodeProcess.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulong)]
        kernel.CloseHandle.argtypes = [ctypes.c_void_p]
        handle = kernel.OpenProcess(0x1000, False, pid)
        if not handle:
            return ctypes.get_last_error() != 87  # access-denied is conservative
        code = ctypes.c_ulong()
        try:
            if not kernel.GetExitCodeProcess(handle, ctypes.byref(code)):
                return True
            return code.value == 259
        finally:
            kernel.CloseHandle(handle)
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def cached_job(jobs, key):
    # Keep incomplete evidence intact. One bounded retry is allowed only after
    # its recorded process is gone; never overwrite or kill an unknown process.
    for suffix in ("", "-retry"):
        job = jobs / (key[:20] + suffix)
        if not job.exists() or (job / "result.json").exists():
            return job
        running = job / "running.json"
        if running.exists() and active_pid(json.loads(running.read_text())["pid"]):
            raise RuntimeError("Owned recorded PID is still active; resume after it finishes: " + str(job))
    raise RuntimeError("Interrupted-job retry budget exhausted for " + key)


def pipeline_signature(source, policy_update=False):
    # Explicit scheduler-only checkpoint migration: the entire fixed candidate
    # flow, proposal policy, scoring and module constants must remain identical.
    excluded = {"main", "pipeline_signature", "wait_receipt"}
    if policy_update:
        excluded |= {"portfolio", "propose"}
    nodes = [n for n in ast.parse(source).body
             if isinstance(n, ast.Assign) or isinstance(n, ast.FunctionDef) and n.name not in excluded]
    return digest("\n".join(ast.dump(n, include_attributes=False) for n in nodes).encode())


def wait_receipt(job, deadline):
    # A recorded adoption monitor owns the already-running tool process. This
    # worker counts its occupied slot without launching another physical job.
    receipt = job / "result.json"
    while not receipt.exists():
        if time.time() >= deadline:
            raise RuntimeError("Adopted job has not produced a receipt: " + str(job))
        time.sleep(1)
    return json.loads(receipt.read_text())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True, help="Immutable audited original-order export")
    parser.add_argument("--output", type=Path, required=True, help="Independent directory below this worktree's target")
    parser.add_argument("--gates", type=Path, required=True, help="Passed normalisation/simulation receipt")
    parser.add_argument("--import-result", type=Path, action="append", default=[])
    parser.add_argument("--workers", type=int, choices=(1, 2, 4), default=2)
    parser.add_argument("--max-candidates", type=int, default=16)
    parser.add_argument("--minutes", type=float, default=90)
    parser.add_argument("--stagnation", type=int, default=8)
    parser.add_argument("--seed", type=int, default=20261029)
    parser.add_argument("--scheduler-update", action="store_true", help="Migrate an existing checkpoint only if its frozen candidate pipeline is identical")
    parser.add_argument("--policy-update", action="store_true", help="Explicit proposal-only update with unchanged candidate validation/build/scoring")
    parser.add_argument("--previous-tool", type=Path, help="Exact previous script for an authorised control/policy update")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[3]
    base, output = args.base.resolve(), args.output.resolve()
    output.relative_to(repo / "target")
    if args.minutes < 25 or not 4 <= args.max_candidates <= 64 or args.stagnation < 1:
        parser.error("Require >=25 minutes, 4..64 candidates, positive stagnation budget")
    output.mkdir(parents=True, exist_ok=True)
    # OS advisory lock is released after a crash. A second live orchestrator may
    # not share this checkpoint. Partial jobs are retained, never overwritten.
    lock = (output / "stage.lock").open("a+b")
    lock.write(b"0"); lock.flush(); lock.seek(0)
    if os.name == "nt":
        import msvcrt
        msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
    else:
        import fcntl
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    vendor = Path(os.environ["GOWIN_HOME"]) / "IDE/simlib/gw2a/prim_sim.v"
    gates = json.loads(args.gates.read_text())
    if not gates.get("passed") or gates["original_inputs"] != inputs(base):
        raise ValueError("Simulation gate missing or source identity differs")
    normal = output / "normalized"
    if not normal.exists():
        normal.mkdir()
        shutil.copytree(base / "src", normal / "src")
        shutil.copyfile(base / "build.tcl", normal / "build.tcl")
        raw = (normal / TOP).read_bytes().decode()
        transformed = permute(raw, NAMES)
        save(normal / "structure.json", structure(raw, transformed))
        (normal / TOP).write_bytes(transformed.encode())
    if inputs(normal) != gates["normalized_inputs"]:
        raise ValueError("Normalised inputs differ from verified simulation gate")
    expected = elaborate(normal, vendor)
    if digest(json.dumps(expected, sort_keys=True).encode()) != gates["connection_sha256"]:
        raise ValueError("Normalisation elaboration differs from gate")
    context = {"source_commit": gates["source_commit"], "original_inputs": inputs(base),
               "normalized_inputs": inputs(normal), "build_tcl_sha256": digest((base / "build.tcl").read_bytes()),
               "gate_receipt_sha256": digest(args.gates.read_bytes()), "tool_script_sha256": digest(Path(__file__).read_bytes()),
               "gowin_sha256": digest((Path(os.environ["GOWIN_HOME"]) / "IDE/bin/gw_sh.exe").read_bytes()),
               "iverilog_sha256": digest(Path(os.environ["IVERILOG_EXE"]).read_bytes()),
               "vendor_sha256": digest(vendor.read_bytes()), "seed": args.seed,
               "workers": args.workers, "max_candidates": args.max_candidates, "minutes": args.minutes,
               "stagnation": args.stagnation, "netlist_hash_usage": "audit only; not an equivalence/deduplication claim"}
    identity = digest(json.dumps(context, sort_keys=True).encode())
    state_path = output / "state.json"
    rng = random.Random(args.seed)
    if state_path.exists():
        state = json.loads(state_path.read_text())
        if state["context_hash"] != identity:
            previous = state["context"]
            changed = {k for k in context.keys() | previous.keys() if context.get(k) != previous.get(k)}
            frozen = args.previous_tool.resolve() if args.previous_tool else output / "frozen-flow.py"
            if (not (args.scheduler_update or args.policy_update) or changed != {"tool_script_sha256"} or not frozen.exists()
                    or digest(frozen.read_bytes()) != previous["tool_script_sha256"]
                    or pipeline_signature(frozen.read_text(), args.policy_update) != pipeline_signature(Path(__file__).read_text(), args.policy_update)):
                raise ValueError("Context/fixed pipeline changed; use a new explicit stage directory")
            state.setdefault("job_context_hash", state["context_hash"])
            state.setdefault("scheduler_updates", []).append({"from_sha256": previous["tool_script_sha256"],
                                                               "to_sha256": context["tool_script_sha256"],
                                                               "fixed_pipeline_sha256": pipeline_signature(frozen.read_text(), args.policy_update),
                                                               "proposal_only_update": args.policy_update,
                                                               "time": time.time()})
            state["context"], state["context_hash"] = context, identity
            save(state_path, state)
        rng.setstate(tuple_tree(state["rng_state"]))
    else:
        started = gates.get("stage_started_at", time.time())
        state = {"context_hash": identity, "context": context, "results": [], "pending": [],
                 "stale": 0, "deadline": started + args.minutes * 60, "rng_state": rng.getstate()}
        for receipt in args.import_result:
            result = json.loads(receipt.read_text())
            project = Path(result["directory"])
            actual = inputs(project)
            wanted = dict(inputs(normal))
            wanted[TOP] = digest(permute((normal / TOP).read_bytes().decode(), result["order"]).encode())
            if actual != wanted or digest((project / "build.tcl").read_bytes()) != context["build_tcl_sha256"]:
                raise ValueError("Imported experiment input mismatch")
            if json.loads((project / "elaborated-connections.json").read_text()) != expected:
                raise ValueError("Imported elaboration mismatch")
            result.update(metrics(project))
            classify(result, project, swept(base), synthesis_diagnostics(base))
            save(project / "reclassified-result.json", result)
            state["results"].append(result)
        save(state_path, state)
    jobs = output / "jobs"
    jobs.mkdir(exist_ok=True)
    # Completion-first scheduling keeps both slots occupied. The checkpoint saves
    # each proposal/RNG state and actual completion order, rather than pretending
    # a fixed seed alone makes asynchronous wall-clock ordering deterministic.
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        active = {}
        job_identity = state.get("job_context_hash", identity)
        for order in state["pending"]:
            job = jobs / cache_key(job_identity, order)[:20]
            if (job / "adopted.json").exists() and not (job / "result.json").exists():
                future = pool.submit(wait_receipt, job, state["deadline"] - 1200)
            else:
                job = cached_job(jobs, cache_key(job_identity, order))
                if (job / "result.json").exists():
                    future = concurrent.futures.Future()
                    future.set_result(json.loads((job / "result.json").read_text()))
                else:
                    future = pool.submit(run_job, normal, job, order, vendor, expected, state["deadline"] - 1200, swept(base), synthesis_diagnostics(base))
            active[future] = order
        while True:
            while (len(active) < args.workers and len(state["results"]) + len(active) < args.max_candidates
                   and not state.get("stop_requested") and state["stale"] < args.stagnation
                   and time.time() < state["deadline"] - 1200):
                seen = {tuple(r["order"]) for r in state["results"]} | {tuple(o) for o in state["pending"]}
                order = propose(rng, state["results"], seen, state["stale"])
                state["pending"].append(order)
                state["rng_state"] = rng.getstate()
                save(state_path, state)
                job = cached_job(jobs, cache_key(job_identity, order))
                active[pool.submit(run_job, normal, job, order, vendor, expected, state["deadline"] - 1200, swept(base), synthesis_diagnostics(base))] = order
            if not active:
                break
            completed, _ = concurrent.futures.wait(active, timeout=5, return_when=concurrent.futures.FIRST_COMPLETED)
            for future in sorted(completed, key=lambda f: tuple(active[f])):
                best = min((r["score"] for r in state["results"] if r.get("score") is not None), default=None)
                result = future.result()
                if result["score"] is not None:
                    state["stale"] = 0 if best is None or tuple(result["score"]) < tuple(best) else state["stale"] + 1
                state["results"].append(result)
                state["pending"].remove(active.pop(future))
                if state["stale"] >= args.stagnation:
                    state["stop_requested"] = "stagnation"
                save(state_path, state)
                print(json.dumps({"completed": len(state["results"]), "order": result["order"],
                                  "valid": result["valid"], "qualified": result["qualified"], "score": result["score"],
                                  "area": result.get("area"), "wns": result.get("setup_wns_ns"),
                                  "tns": result.get("setup_tns_ns"), "seconds": result["elapsed_seconds"]}), flush=True)
    ranked = portfolio(state["results"])
    state["best"] = ranked[0] if ranked else None
    state["stop_reason"] = ("candidate_budget" if len(state["results"]) >= args.max_candidates else
                            "stagnation" if state.get("stop_requested") or state["stale"] >= args.stagnation else "time_budget")
    save(state_path, state)
    print(json.dumps({"phase": "search_complete", "stop_reason": state["stop_reason"],
                      "best_order": state["best"]["order"] if state["best"] else None,
                      "best_score": state["best"]["score"] if state["best"] else None}), flush=True)
    # Independent fresh synthesis/PnR, with exactly the winning HDL and settings.
    if state["best"] and state["best"]["qualified"] and "reproduction" not in state:
        repeat_dir = output / "reproduction"
        if repeat_dir.exists():
            repeat = json.loads((repeat_dir / "result.json").read_text())
        else:
            repeat = run_job(normal, repeat_dir, state["best"]["order"], vendor, expected, state["deadline"], swept(base), synthesis_diagnostics(base))
        state["reproduction"] = repeat
        fields = ("inputs", "area", "setup_violations", "hold_violations", "setup_wns_ns", "hold_wns_ns", "setup_tns_ns", "hold_tns_ns")
        state["reproduced"] = bool(repeat["qualified"] and all(repeat.get(k) == state["best"].get(k) for k in fields))
    save(state_path, state)
    print(json.dumps({"stop_reason": state["stop_reason"], "completed": len(state["results"]),
                      "best": state["best"]["score"] if state["best"] else None,
                      "reproduced": state.get("reproduced", False)}), flush=True)
    lock.close()


if __name__ == "__main__":
    main()
