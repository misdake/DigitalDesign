"""Bounded offline two-edge stage search; use lighting_two_cycle_probe to check.

No numerical graph, fusion, or production schedule is changed. Counted connection
and retention metrics are proxies, never fitted Logic/FF numbers.
"""
import argparse
import csv
import json
import math
import random
from collections import defaultdict
from pathlib import Path

DSP = ("SmallMultiply", "LargeMultiply", "PairMultiplyAdd")


def rows(path):
    with path.open(encoding="utf-8", newline="") as stream:
        return list(csv.DictReader(stream))


def integers(text):
    return [int(value) for value in text.split(";") if value]


class Case:
    def __init__(self, path, rom_limit=6):
        self.rom_limit = rom_limit
        self.name = path.name
        self.resources = rows(path / "resources.csv")
        self.nodes = {int(n["id"]): n for n in rows(path / "nodes.csv")}
        self.parents = {i: integers(n["parents"]) for i, n in self.nodes.items()}
        self.children = {i: [] for i in self.nodes}
        for i, parents in self.parents.items():
            for parent in parents:
                self.children[parent].append(i)
        self.order = []
        seen = set()
        while len(seen) != len(self.nodes):
            before = len(seen)
            for i in self.nodes:
                if i not in seen and all(p in seen for p in self.parents[i]):
                    self.order.append(i)
                    seen.add(i)
            assert len(seen) > before, "cyclic bound graph"
        self.kind = {i: self.resources[int(n["resource"])]["name"] for i, n in self.nodes.items()}
        self.latency = {i: int(self.resources[int(n["resource"])]["latency"]) for i, n in self.nodes.items()}
        self.baseline = {int(s["id"]): int(s["issue"]) for s in rows(path / "baseline.csv")}
        self.ports = rows(path / "ports.csv")
        self.asap = {}
        for i in self.order:
            self.asap[i] = max((self.asap[p] + self.latency[p] for p in self.parents[i]), default=0)

    def counts(self, times, window):
        groups = defaultdict(lambda: [0, 0])
        for i, time in times.items():
            if self.kind[i] in DSP:
                groups[(self.kind[i], time // (2 * window) if window else 0)][time % 2] += 1
        count = {kind: 0 for kind in DSP}
        for (kind, _), phases in groups.items():
            count[kind] += max(phases)
        return count

    def retention(self, times):
        # Width-weighted duration of bound results. Multi-output cone internals,
        # input capture, DSP registers and control are excluded intentionally.
        return sum(
            int(n["bits"]) * max(0, max((times[c] for c in self.children[i]), default=times[i] + self.latency[i]) - times[i] - self.latency[i])
            for i, n in self.nodes.items() if n["stable"] == "false"
        )

    def span(self, times):
        return max(times[i] + self.latency[i] for i in times)

    def score(self, times, window):
        count = self.counts(times, window)
        # Charge macro granularity first, then extra channels, then lifetimes.
        macros = (count[DSP[0]] + 3) // 4 + (count[DSP[1]] + 1) // 2 + count[DSP[2]]
        rom_phases = [sum(self.kind[i] == "NormalizeRead" and time % 2 == phase for i, time in times.items()) for phase in (0, 1)]
        return (max(0, max(rom_phases) - self.rom_limit), macros, sum(count.values()), self.retention(times), self.span(times))

    def search(self, window, slack, seed):
        rng = random.Random(seed)
        times = self.asap.copy()
        deadline = self.span(self.asap) + slack
        # Bounded whole-branch perturbations escape a fully packed ASAP graph:
        # delaying one multiply repairs every affected descendant before scoring.
        movable = [i for i in self.order if self.kind[i] in DSP]
        best_times = times.copy()
        def energy(value):
            rom_excess, macros, channels, retained, span = self.score(value, window)
            return rom_excess * 1800 + macros * 1000 + channels * 60 + retained / 100 + span / 10
        current_energy = energy(times)
        for step in range(1600):
            trial = times.copy()
            i = rng.choice(movable)
            low = max((trial[p] + self.latency[p] for p in self.parents[i]), default=0)
            trial[i] = max(low, trial[i] + rng.choice((-4, -2, -1, 1, 2, 3, 4)))
            for child in self.order:
                earliest = max((trial[p] + self.latency[p] for p in self.parents[child]), default=0)
                trial[child] = max(trial[child], earliest)
            if self.span(trial) > deadline:
                continue
            value = energy(trial)
            temperature = 160 * (1 - step / 1600) + 3
            if value < current_energy or rng.random() < math.exp(min(0, (current_energy - value) / temperature)):
                times, current_energy = trial, value
            if self.score(times, window) < self.score(best_times, window):
                best_times = times.copy()
        times = best_times
        # Move every cone within a bounded ASAP/ALAP interval. This lets an early
        # branch move as a whole, without changing any atomic fusion boundary.
        for sweep in range(24):
            changed = False
            order = self.order.copy()
            if sweep % 3 == 0:
                order.reverse()
            elif sweep % 3 == 1:
                rng.shuffle(order)
            for i in order:
                low = max((times[p] + self.latency[p] for p in self.parents[i]), default=0)
                high = min((times[c] - self.latency[i] for c in self.children[i]), default=deadline - self.latency[i])
                old = times[i]
                best, choices = self.score(times, window), [old]
                for time in range(low, min(high, low + 16) + 1):
                    times[i] = time
                    score = self.score(times, window)
                    if score < best:
                        best, choices = score, [time]
                    elif score == best:
                        choices.append(time)
                times[i] = rng.choice(choices)
                changed |= times[i] != old
            if not changed:
                break
        return times

    def assign(self, times, window):
        # Each DSP lane belongs to one stage/window in this mode. In each phase
        # it executes at most one operation; repeated pixels start every 2 edges.
        capacities = [1] * len(self.resources)
        slots = []
        groups = defaultdict(lambda: [[], []])
        for i in self.order:
            r = int(self.nodes[i]["resource"])
            group = times[i] // (2 * window) if self.kind[i] in DSP and window else 0
            groups[(r, group)][times[i] % 2].append(i)
        offsets = defaultdict(int)
        for (r, group), phases in sorted(groups.items()):
            for phase in phases:
                # Fixed node order makes lane allocation reproducible.
                for lane, i in enumerate(sorted(phase)):
                    slots.append({"id": i, "issue": times[i], "lane": offsets[r] + lane})
            offsets[r] += max(map(len, phases))
            capacities[r] = offsets[r]
        return slots, capacities

    def save(self, directory, times, window):
        directory.mkdir(parents=True, exist_ok=True)
        slots, capacities = self.assign(times, window)
        with (directory / "slots.csv").open("w", newline="", encoding="utf-8") as stream:
            writer = csv.DictWriter(stream, fieldnames=("id", "issue", "lane"))
            writer.writeheader()
            writer.writerows(sorted(slots, key=lambda s: s["id"]))
        (directory / "capacities.txt").write_text(" ".join(map(str, capacities)), encoding="utf-8")
        return {"mode": self.name, "span": self.span(times), "dsp": self.counts(times, window), "result_bit_cycles": self.retention(times), "stages": (self.span(times) + 1) // 2}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--align-prefix", type=Path)
    parser.add_argument("--unified-round", type=int, choices=(1, 2))
    args = parser.parse_args()
    root = args.root
    cases = [Case(root / "input" / name) for name in ("floor-full", "floor-diffuse", "rne-full", "rne-diffuse")]
    if args.unified_round:
        results=[]
        # Same full numerical DAG and throughput; only physical locality differs.
        for window, slack in ((0,0),(0,4),(0,8),(1,4),(2,0),(2,4),(2,8)):
            name=f"round{args.unified_round}-{'free' if window==0 else 'group'+str(window)}-slack{slack}"
            record={'candidate':name,'window':window,'cases':[]}
            for case in (cases[0],cases[2]):
                seed_base=(args.unified_round-1)*32
                choices=[case.search(window,slack,seed_base+seed) for seed in range(3 if args.unified_round==1 else 6)]
                best=min(choices,key=lambda times:case.score(times,window))
                record['cases'].append(case.save(root/name/case.name,best,window))
                other=root/name/case.name.replace('-full','-diffuse')
                other.mkdir(parents=True,exist_ok=True)
                for file in ('slots.csv','capacities.txt'):
                    (other/file).write_bytes((root/name/case.name/file).read_bytes())
            results.append(record)
            print(name,json.dumps(record['cases']),flush=True)
        (root/f'round{args.unified_round}-search.json').write_text(json.dumps(results,indent=2),encoding='utf-8')
        return
    if args.align_prefix:
        source = args.align_prefix
        for full, diffuse in ((cases[0], cases[1]), (cases[2], cases[3])):
            full_slots = rows(source / full.name / "slots.csv")
            full_by_id = {int(s["id"]): s for s in full_slots}
            shared = {i for i, n in diffuse.nodes.items() if i in full.nodes and n["recipe"] == full.nodes[i]["recipe"]}
            assert len(shared) == len(diffuse.nodes) - 1, "diffuse must be the verified full prefix plus commit"
            times = {}
            slots = []
            for i in diffuse.order:
                time = int(full_by_id[i]["issue"]) if i in shared else max(times[p] + diffuse.latency[p] for p in diffuse.parents[i])
                times[i] = time
                slots.append({"id": i, "issue": time, "lane": int(full_by_id[i]["lane"]) if i in shared else 0})
            out = root / "aligned-prefix" / diffuse.name
            out.mkdir(parents=True, exist_ok=True)
            capacities = [1] * len(diffuse.resources)
            for s in slots:
                r = int(diffuse.nodes[s["id"]]["resource"])
                capacities[r] = max(capacities[r], s["lane"] + 1)
            with (out / "slots.csv").open("w", newline="", encoding="utf-8") as stream:
                writer = csv.DictWriter(stream, fieldnames=("id", "issue", "lane"));writer.writeheader();writer.writerows(slots)
            (out / "capacities.txt").write_text(" ".join(map(str, capacities)), encoding="utf-8")
            other = root / "aligned-prefix" / full.name
            other.mkdir(parents=True, exist_ok=True)
            for name in ("slots.csv", "capacities.txt"):
                (other / name).write_bytes((source / full.name / name).read_bytes())
            print(full.name, "shared prefix", len(shared), "diffuse span", diffuse.span(times))
        return
    results = []
    for window, slack in ((1, 0), (1, 4), (1, 8), (2, 0), (2, 4), (3, 4)):
        name = f"window{window}-slack{slack}"
        result = {"candidate": name, "window_stages": window, "max_extra_edges": slack, "cases": []}
        for case in cases:
            candidates = [case.search(window, slack, seed) for seed in range(6)]
            best = min(candidates, key=lambda t: case.score(t, window))
            result["cases"].append(case.save(root / name / case.name, best, window))
        for policy in ("floor", "rne"):
            pair = [c for c in result["cases"] if c["mode"].startswith(policy)]
            count = {kind: max(c["dsp"][kind] for c in pair) for kind in DSP}
            macros = (count[DSP[0]] + 3) // 4 + (count[DSP[1]] + 1) // 2 + count[DSP[2]]
            result[policy] = {"dsp": count, "macros": macros, "tiles": (macros + 1) // 2}
        results.append(result)
        print(name, json.dumps({p: result[p] for p in ("floor", "rne")}), flush=True)
    (root / "search.json").write_text(json.dumps(results, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
