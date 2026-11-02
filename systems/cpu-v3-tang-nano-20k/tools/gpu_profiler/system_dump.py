"""Compiled-program system capture -> the same GPS1 snapshot used by UART."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
from snapshot import Abi, SnapshotError, bytes_to_words, decode_stream, encode_reference


def materialize(raw, output, provenance, abi=None):
    """Re-validate driver bytes through the shared wire codec, not a second schema."""
    raw, output, abi = Path(raw), Path(output), abi or Abi()
    origin = json.loads((raw / "origin.json").read_text())
    if (origin.get("schema") != 1 or origin.get("source") != "system-simulation"
            or origin.get("boundary") != "before-profiler-gate"
            or origin.get("gpu_consumed") is not False
            or origin.get("prefix_gpu", "reject") not in ("reject", "functional")):
        raise SnapshotError("System dump has no pre-consumption boundary")
    cpu_mode = origin.get("cpu_mode", "instruction")
    if cpu_mode not in ("instruction", "native"):
        raise SnapshotError("Unknown system CPU backend")
    if cpu_mode == "native" and (origin.get("cpu_steps") is not None
            or any(type(origin.get(key)) is not int or origin[key] <= 0
                   for key in ("cpu_logic_cycles", "native_cycles"))):
        raise SnapshotError("Native system dump requires modeled clocks, not CPU steps")
    header = bytes_to_words((raw / "header.bin").read_bytes())
    decoded = abi.header(header)
    ranges = bytes_to_words((raw / "ranges.bin").read_bytes())
    count, width = decoded["range_count"], abi["RANGE_WORDS"]
    if len(ranges) != count * width or origin.get("range_count") != count:
        raise SnapshotError("Incomplete system dump range table")
    names = {f"range-{i:03d}.bin" for i in range(count)}
    if {p.name for p in raw.glob("range-*.bin")} != names:
        raise SnapshotError("Missing or unexpected system range payload")
    rows = [ranges[i * width:(i + 1) * width] for i in range(count)]
    blobs = [(raw / name).read_bytes() for name in sorted(names)]
    wire = encode_reference(header, rows, blobs, abi)
    attempts, validation = decode_stream(wire, abi)
    if len(attempts) != 1 or validation["failures"]:
        raise SnapshotError("System dump did not form one complete GPS1 attempt")
    manifest = attempts[0].save(output)
    manifest['board_dump_ms'] = None
    manifest['identity_note'] = 'System simulation only; compiled-program provenance is in source.json.'
    (output / 'snapshot.json').write_text(json.dumps(manifest, indent=2) + '\n')
    # Provenance differs across frontends; descriptor/ranges/payload semantics do not.
    (output / "source.json").write_text(json.dumps(dict(
        **origin, **provenance, input_sha256=manifest["input_sha256"],
        replay_scope="functional cold replay; no CPU/Display contention or board timing"
    ), indent=2) + "\n")
    return manifest


def capture(recipe_path, frontend, output):
    recipe_path, output = Path(recipe_path).resolve(), Path(output).resolve()
    recipe = json.loads(recipe_path.read_text())
    if recipe.get("schema") != 1 or recipe.get("format") != "cb06":
        raise SnapshotError("Unsupported compiled-program capture recipe")
    prefix_gpu = recipe.get("prefix_gpu", "reject")
    if prefix_gpu not in ("reject", "functional"):
        raise SnapshotError("prefix_gpu must be reject or functional")
    cpu_mode = recipe.get("cpu_mode", "instruction")
    if cpu_mode not in ("instruction", "native"):
        raise SnapshotError("cpu_mode must be instruction or native")
    program = (recipe_path.parent / recipe["program"]["file"]).resolve()
    digest = hashlib.sha256(program.read_bytes()).hexdigest()
    if digest != recipe["program"]["sha256"]:
        raise SnapshotError("Compiled program identity differs from capture recipe")
    budget_key = "max_cycles" if cpu_mode == "native" else "max_steps"
    wrong_budget = "max_steps" if cpu_mode == "native" else "max_cycles"
    if wrong_budget in recipe:
        raise SnapshotError("CPU backend and work budget differ: " + wrong_budget)
    if type(recipe.get(budget_key)) is not int or recipe[budget_key] <= 0:
        raise SnapshotError("System capture needs a positive " + budget_key)
    timeout = recipe["timeout_seconds"]
    if not isinstance(timeout, (int, float)) or timeout <= 0:
        raise SnapshotError("System capture needs a positive wall-time bound")
    output.mkdir(parents=True, exist_ok=False)
    args = [str(Path(frontend).resolve()), "--flash", str(program), "--out", str(output / "raw")]
    for argument, value in [("gate-pc", recipe["capture_point"]["gate_pc"]),
                            ("header-word", recipe["capture_point"]["header_word"]),
                            ("ranges-word", recipe["capture_point"]["ranges_word"]),
                            ("memory-seed", recipe["memory_seed"]),
                            ("max-cycles" if cpu_mode == "native" else "max-steps", recipe[budget_key])]:
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise SnapshotError("Invalid capture recipe integer: " + argument)
        args += ["--" + argument, str(value)]
    if cpu_mode == "native":
        args += ["--cpu-mode", "native"]
    if prefix_gpu == "functional":
        args += ["--prefix-gpu", "functional"]
    try:
        with (output / "frontend.log").open("wb") as log:
            result = subprocess.run(args, stdout=log, stderr=subprocess.STDOUT, timeout=timeout)
        if result.returncode:
            raise SnapshotError("System frontend failed; see frontend.log")
        origin = json.loads((output / "raw" / "origin.json").read_text())
        if origin.get("cpu_mode", "instruction") != cpu_mode:
            raise SnapshotError("System frontend CPU backend differs from recipe")
        if origin.get("prefix_gpu", "reject") != prefix_gpu:
            raise SnapshotError("System frontend prefix GPU mode differs from recipe")
        return materialize(output / "raw", output / "snapshot", dict(
            compiled_program_sha256=digest,
            recipe_sha256=hashlib.sha256(recipe_path.read_bytes()).hexdigest(),
            capture_point=recipe["capture_point"],
        ))
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        (output / "failure.json").write_text(json.dumps(dict(complete=False, error=str(error))) + "\n")
        raise


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("recipe", type=Path)
    parser.add_argument("--frontend", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    manifest = capture(args.recipe, args.frontend, args.out)
    print(json.dumps(dict(complete=manifest['complete'], input_sha256=manifest['input_sha256'],
                          ranges=len(manifest['ranges']), decoded_bytes=manifest['decoded_bytes']), indent=2))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.TimeoutExpired) as error:
        print("System capture refused: " + str(error), file=sys.stderr)
        raise SystemExit(1)
