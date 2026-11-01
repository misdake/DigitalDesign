"""Bounded actual command coefficient-to-weight-bank qualification; no frame or board."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import random
import re
import subprocess
import sys

sys.dont_write_bytecode = True
COUNT = 192


def module(source, name):
    return re.search(r"module\s+" + re.escape(name) + r"(?=[\s(#]).*?\bendmodule\b", source, re.S)[0]


def packed(values, width):
    return sum(value << (i * width) for i, value in enumerate(values))


def weights(parent, u, v):
    bottom = parent * v // 256
    top = parent - bottom
    right_bottom = bottom * u // 256
    right_top = top * u // 256
    return [top - right_top, right_top, bottom - right_bottom, right_bottom]


def vectors(rtt):
    rng = random.Random(80611)
    rows, golden = [], []
    for i in range(COUNT):
        n = 9 if rtt else i % 11
        nearest = i % 5 == 0
        p1 = 0 if nearest or n == 0 or rtt else [0, 1, 255, 256, 510, 511][i % 6]
        p0 = 511 - p1
        side = 1 << max(n, 1)
        x = ([0, 1, 14, 15, 16, 398, 399][i % 7] if rtt else rng.randrange(side))
        y = ([0, 1, 6, 7, 8, 238, 239][i % 7] if rtt else rng.randrange(side))
        if rtt and not nearest:
            x, y = min(x, 398), min(y, 238)
        u, v = (0, 0) if nearest else ([0, 1, 127, 128, 129, 255][i % 6], [255, 128, 1, 127, 0, 129][i % 6])
        # Logical fixed-point half-texel conversion, independent of the RTL muxes.
        cx = ((x * 256 + u - 128) // 2) if n > 1 else x * 256 + u
        cy = ((y * 256 + v - 128) // 2) if n > 1 else y * 256 + v
        cu, cv = (0, 0) if nearest else (cx % 256, cy % 256)
        cmask = (1 << max(n - 1, 1)) - 1
        coords = [x, (x + 1) % side, y, (y + 1) % side,
                  (cx // 256) & cmask, ((cx // 256) + 1) & cmask,
                  (cy // 256) & cmask, ((cy // 256) + 1) & cmask]
        key = i % 64
        fields = [(int(nearest), 0), (p0, 1), (p1, 11), (u, 21), (v, 29),
                  (cu, 37), (cv, 45), (n, 141), (max(n - 1, 0), 145),
                  (0, 149), (key, 154), (int(p1 == 0), 161)]
        fields += [(c, 53 + 11 * j) for j, c in enumerate(coords)]
        rows.append(sum(value << shift for value, shift in fields))
        fine, coarse = weights(p0, u, v), weights(p1, cu, cv)
        meta = ((key << 30) | (n << 22) | (int(not (y & 1) and v < 128) << 21)
                | (int(not (x & 1) and u < 128) << 20) | (y << 10) | x)
        mask = packed([int(w != 0) for w in fine + coarse], 9)
        cost = int(p0 != 0) + int(p1 != 0)
        golden.append(packed(fine, 9) | (packed(coarse, 9) << 36) | (meta << 72)
                      | (mask << 108) | (((key << 1) | (n & 1)) << 180)
                      | (int(n != 0) << 187) | (cost << 188))
    return rows, golden


def compile_dut(directory, source, native, monitors):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "dut.v").write_text(source)
    bench = Path(__file__).with_name("fixtures") / "command_direct_weight_tb.v"
    library = Path(os.environ["GOWIN_HOME"]) / "IDE/simlib/gw2a/prim_sim.v"
    executable = directory / "sim.vvp"
    command = [os.environ["IVERILOG_EXE"], "-g2012", "-s", "tb", f"-Ptb.NATIVE={int(native)}"]
    command += ["-DGPU_V2_GOWIN_DSP"] if native else []
    command += [] if monitors else ["-DSYNTHESIS"]
    command += ["-o", str(executable), str(directory / "dut.v"), str(bench), str(library)]
    result = subprocess.run(command, capture_output=True, text=True, timeout=60)
    (directory / "compile.log").write_text(result.stdout + result.stderr)
    if result.returncode:
        raise RuntimeError(f"compile failed: {directory / 'compile.log'}")
    return executable


def simulate(executable, name, inputs, gold, collision=False):
    command = [os.environ["VVP_EXE"], str(executable), f"+INPUT={inputs}", f"+GOLD={gold}"]
    command += ["+COLLISION"] if collision else []
    result = subprocess.run(command, capture_output=True, text=True, timeout=60)
    log = result.stdout + result.stderr
    (executable.parent / (name + ".log")).write_text(log)
    return result.returncode, log


def run(rtl, baseline, output):
    output.mkdir(parents=True, exist_ok=False)
    cases = []
    for name, rtt in [("pot", False), ("rtt", True)]:
        rows, gold = vectors(rtt)
        inp, expected = output / (name + ".mem"), output / (name + "-gold.mem")
        inp.write_text("".join(f"{value:041x}\n" for value in rows))
        expected.write_text("".join(f"{value:048x}\n" for value in gold))
        cases.append((name, inp, expected))
    rows, traces = [], {}
    for label, path in [("candidate", rtl)] + ([("baseline", baseline)] if baseline else []):
        whole = path.read_text()
        producer = module(whole, "gpu_v2_runtime_coefficient_directweight")
        cache = module(whole, "gpu_v2_texture_runtime_cache_unique")
        shared = module(whole, "gpu_v2_texture_coefficient_mul9")
        wrapper = module(whole, "gpu_v2_texture_runtime_backend_unique")
        assert "assign weight_write=" not in wrapper
        sampling = module(whole, "gpu_v2_vliw_sampling")
        for pin in ["write", "address", "value"]:
            assert wrapper.count(f".direct_weight_{pin}(weight_{pin})") == 1
            assert sampling.count(f".weight_{pin}(weight_{pin})") == 2
        source = producer + "\n" + shared + "\n" + cache
        for native in [False, True]:
            for monitors in [False, True]:
                directory = output / f"{label}-n{int(native)}-m{int(monitors)}"
                executable = compile_dut(directory, source, native, monitors)
                for name, inp, gold in cases:
                    code, log = simulate(executable, name, inp, gold)
                    assert code == 0 and "PASS directweight" in log, str(directory / (name + ".log"))
                    trace = "\n".join(line for line in log.splitlines() if line.startswith(("A ", "W ", "O ")))
                    traces[(label, native, monitors, name)] = trace
                    rows.append({"label": label, "native": native, "monitors": monitors, "case": name,
                                 "trace_sha256": hashlib.sha256(trace.encode()).hexdigest(),
                                 "result": next(line for line in log.splitlines() if line.startswith("PASS"))})
                code, log = simulate(executable, "collision", cases[0][1], cases[0][2], True)
                assert ((code != 0 and "coefficient metadata cross-port collision" in log) if monitors
                        else code == 0 and "PASS collision" in log), str(directory / "collision.log")
    for name, _, _ in cases:
        reference = traces[("candidate", False, False, name)]
        assert all(trace == reference for key, trace in traces.items() if key[3] == name), name
    # Mutate real DUT producer/bank logic, not oracle inputs or expected answers.
    original = rtl.read_text()
    p = module(original, "gpu_v2_runtime_coefficient_directweight")
    c = module(original, "gpu_v2_texture_runtime_cache_unique")
    m = module(original, "gpu_v2_texture_coefficient_mul9")
    mutations = [
        ("wrong-key", p.replace("weight_key1<=weight_key0;", "weight_key1<={(lv0==0),lv0[0],key_in[5:0]};"), c),
        ("wrong-plane", p.replace("weight_key1[6]^(v4&&!weight_key1[7])", "weight_key1[6]"), c),
        ("wrong-value", p.replace("assign direct_weight_value=returning_corners;", "assign direct_weight_value=returning_corners^36'd1;"), c),
        ("duplicate-writer", p, c.replace(".CEA(weight_write)", ".CEA(weight_write||proposal_ce)")),
    ]
    negatives = []
    for name, producer, cache in mutations:
        assert producer != p or cache != c, name
        exe = compile_dut(output / name, producer + "\n" + m + "\n" + cache, True, True)
        code, log = simulate(exe, "negative", cases[0][1], cases[0][2])
        assert code != 0 and "FATAL" in log and any(t in log for t in ["weight value/address", "sole writer/count"]), name
        negatives.append({"name": name, "caught": True})
    result = {"passed": True, "rtl_sha256": hashlib.sha256(rtl.read_bytes()).hexdigest(),
              "cases": rows, "negatives": negatives, "cycle_limit": 12000,
              "scope": "Actual producer and unchanged actual cache weight-bank writes/storage; cache request/retirement not exercised."}
    (output / "receipt.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"passed": True, "replays": len(rows), "inputs_per_replay": COUNT, "negative_count": len(negatives)}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rtl", type=Path, required=True)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    run(args.rtl, args.baseline, args.output)
