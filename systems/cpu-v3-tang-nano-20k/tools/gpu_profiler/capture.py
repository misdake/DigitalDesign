"""Bounded Icarus runner; the manifest owns the harness, not the profiler."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

RESERVED_NAMES = frozenset({
    "profiler_tb.v", "capture.json", "capture.vvp", "compile.log", "run.log",
    "events.txt", "load.csv",
})


def run(manifest_path, out, compile_only=False):
    manifest_path, out = Path(manifest_path).resolve(), Path(out).resolve()
    config = json.loads(manifest_path.read_text(encoding="utf-8"))
    if config.get("version") != 1:
        raise ValueError("Unsupported capture manifest")
    limit, timeout = config["max_cycles"], config["timeout_seconds"]
    if type(limit) is not int or not 0 < limit <= 100_000_000:
        raise ValueError("A positive bounded max_cycles is required")
    if type(timeout) is not int or not 0 < timeout <= 3600:
        raise ValueError("A positive wall timeout <=3600 seconds is required")
    if type(config["submissions"]) is not int or not 0 < config["submissions"] <= 10000:
        raise ValueError("A bounded submission count is required")
    if type(config["clock_hz"]) is not int or config["clock_hz"] <= 0:
        raise ValueError("clock_hz must be explicit")
    if out.exists() and any(out.iterdir()):
        raise ValueError("Capture output must be a new/empty directory")
    out.mkdir(parents=True, exist_ok=True)
    sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    resolve = lambda p: (manifest_path.parent / p).resolve()
    sources = []
    for item in config["sources"]:
        item = {"path": item} if isinstance(item, str) else item
        source = resolve(item["path"])
        if source.name.casefold() in RESERVED_NAMES:
            raise ValueError("Source uses a reserved capture filename: " + source.name)
        if item.get("sha256") and sha(source) != item["sha256"]:
            raise ValueError("Source identity mismatch: " + str(source))
        destination = out / source.name
        if destination.exists():
            raise ValueError("Source basename collision: " + source.name)
        shutil.copyfile(source, destination)
        sources.append(destination)
    inputs = {}
    for name, path in config.get("inputs", {}).items():
        if (Path(name).name != name or name.casefold() in RESERVED_NAMES
                or (out / name).exists()):
            raise ValueError("Input must have a unique flat filename")
        shutil.copyfile(resolve(path), out / name)
        inputs[name] = sha(out / name)
    bindings = {"GPU": "gpu", "CLOCK": "clk", "RESET": "reset", "ENABLE": "ce", "CYCLE": "cycle"}
    bindings.update(config.get("bindings", {}))
    for value in [*bindings.values(), config.get("top", "tb")]:
        if not re.fullmatch(r"[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*", value):
            raise ValueError("Bindings must be explicit HDL identifiers")
    observer_path = Path(__file__).with_name("observer.vh")
    observer = observer_path.read_text(encoding="utf-8")
    for key, value in {**bindings, "EVENTS": (out / "events.txt").as_posix(),
                       "LOAD": (out / "load.csv").as_posix()}.items():
        if '"' in value or "\n" in value:
            raise ValueError("Invalid observer path")
        observer = observer.replace("@" + key + "@", value)
    bench = resolve(config["testbench"]).read_text(encoding="utf-8")
    marker = "// GPU_PROFILER_OBSERVER"
    if bench.count(marker) != 1:
        raise ValueError("Place one " + marker + " inside the testbench module")
    tb = out / "profiler_tb.v"
    tb.write_text(bench.replace(marker, observer), encoding="utf-8")
    sources.append(tb)
    metadata = {**config, "manifest_sha256": sha(manifest_path),
                "source_sha256": {p.name: sha(p) for p in sources},
                "observer_sha256": sha(observer_path), "inputs_sha256": inputs}
    (out / "capture.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    command = [os.environ.get("IVERILOG_EXE", "iverilog"), "-g2012", "-D__ICARUS__",
               "-s", config.get("top", "tb"), "-o", str(out / "capture.vvp"), *map(str, sources)]
    result = subprocess.run(command, cwd=out, capture_output=True, timeout=min(timeout, 90))
    (out / "compile.log").write_bytes(result.stdout + result.stderr)
    if result.returncode:
        raise RuntimeError("Icarus compile failed; see " + str(out / "compile.log"))
    if compile_only:
        return
    with (out / "run.log").open("wb") as log:
        result = subprocess.run([os.environ.get("VVP_EXE", "vvp"), str(out / "capture.vvp"),
            f"+PROFILE_LIMIT={limit}", f"+PROFILE_SUBMISSIONS={config['submissions']}",
            "+PROFILE_FINISH=1"], cwd=out, stdout=log, stderr=subprocess.STDOUT, timeout=timeout)
    if result.returncode:
        raise RuntimeError("RTL capture failed; see " + str(out / "run.log"))
    print("Capture saved: " + str(out))
