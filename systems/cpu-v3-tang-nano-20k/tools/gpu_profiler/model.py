"""Versioned GPU profiling sessions and observer-event correlation."""
from bisect import bisect_right
from collections import defaultdict
import copy
import csv
import hashlib
import json
from pathlib import Path

SCHEMA = "digitaldesign.gpu-profile"
VERSION = 2
MAX_INTEGER = (1 << 53) - 1

OP = ["NOP", "DMA_LOCAL", "DMA_MESHLET", "DRAW", "WAIT", "SET_GLOBAL",
      "SET_RENDER", "FLUSH", "CLEAR", "TEX_INVALIDATE", "SET_TARGET", "UI_GRID",
      "RESERVED12", "RESERVED13", "RESERVED14", "END"]
VT_KIND = {1: "V", 2: "T", 3: "T queue wait", 4: "Control / RAM / index", 5: "CE pause"}
METRICS = [
    ("dma", "Command DMA occupied", "binary", 1),
    ("vt_owned", "VT owned by meshlet", "binary", 1),
    ("vt_step", "VT instruction step", "binary", 1),
    ("t_queue", "T result queue", "queue", 16),
    ("prepare_busy", "Triangle preparer occupied", "binary", 1),
    ("r_busy", "Raster occupied", "binary", 1),
    ("i_busy", "Interpolator returns live", "binary", 1),
    ("r_quad", "R output", "rate", 0),
    ("i_pixel", "I depth returns", "rate", 0),
    ("quad_total", "Quad pool allocated", "queue", 24),
    ("quad_producer", "Quad pool unpublished", "queue", 8),
    ("light_pending", "Quads waiting for Light input", "queue", 24),
    ("light_tokens", "Light pixels in flight", "queue", 0),
    ("light_busy", "Light tokens live", "binary", 1),
    ("light_tick", "Light arithmetic advances", "binary", 1),
    ("sample_pending", "Quads waiting for Sample input", "queue", 24),
    ("sample_quads", "Sampling quads in flight", "queue", 16),
    ("sample_busy", "Sampling tokens live", "binary", 1),
    ("sample_tick", "Texture coordinate issues", "binary", 1),
    ("texture_public", "Sampling public owners", "queue", 16),
    ("texture_work", "Texture work queue", "queue", 16),
    ("texture_reserved", "Texture reserved work", "queue", 16),
    ("final_wait", "Published quads to Final cursor", "queue", 24),
    ("final_tokens", "Final pixel tokens", "queue", 2),
    ("rop_contexts", "ROP cache contexts", "queue", 2),
    ("light_issue", "Light pixel issues", "rate", 0),
    ("sample_issue", "Sampling lane issues", "rate", 0),
    ("final_issue", "Final pixel issues", "rate", 0),
    ("rop_lane", "ROP lane accepts", "rate", 0),
    ("fb_hint", "Framebuffer hint queue", "queue", 2),
    ("fb_maintenance", "Framebuffer maintenance live", "binary", 1),
    ("ro_wait", "DMA / texture read backpressure", "binary", 1),
    ("fb_read_wait", "Framebuffer read backpressure", "binary", 1),
    ("fb_write_wait", "Framebuffer request backpressure", "binary", 1),
]


def union(spans):
    result = []
    for start, end in sorted(spans):
        if end <= start:
            continue
        if result and start <= result[-1][1]:
            result[-1][1] = max(result[-1][1], end)
        else:
            result.append([start, end])
    return result



def ensure(condition, message):
    if not condition:
        raise ValueError(str(message))


def build_capture(folder, allow_partial=False):
    folder=Path(folder)
    metadata=json.loads((folder/"capture.json").read_text(encoding="utf-8"))
    for group in ("source_sha256", "inputs_sha256"):
        for name, expected in metadata.get(group, {}).items():
            ensure(Path(name).name == name, "Capture evidence filename")
            ensure(hashlib.sha256((folder / name).read_bytes()).hexdigest() == expected,
                   "Capture evidence changed: " + name)
    events=defaultdict(list)
    cpu=[]; cpu_open={}
    event_text = (folder/"events.txt").read_text(encoding="utf-8")
    lines = event_text.splitlines()
    if allow_partial and not event_text.endswith("\n"):
        lines = lines[:-1]
    for line in lines:
        tokens=line.split()
        if not tokens: continue
        if tokens[0]=="CPU_BEGIN":
            _,cycle,span,frame,label=line.split(maxsplit=4)
            if int(span) in cpu_open: raise ValueError("Duplicate CPU span")
            cpu_open[int(span)]={"id":int(span),"frame_id":int(frame),"name":label,"start":int(cycle)}
        elif tokens[0]=="CPU_END":
            span=cpu_open.pop(int(tokens[2]))
            span["end"]=int(tokens[1]);cpu.append(span)
        else:
            values=[int(v,16 if tokens[0] in ("CMD_EXEC","CMD_END") and i==len(tokens)-2 else 10) for i,v in enumerate(tokens[1:])]
            events[tokens[0]].append(values)
    if events["FORMAT"]!=[[2]]: raise ValueError("Unsupported observer format")
    if not events["SUBMIT"]: raise ValueError("No accepted submission")
    origin=metadata.get("origin",0)
    stop=max(v[0] for k,rows in events.items() if k!="FORMAT" for v in rows)
    complete=bool(events["CAPTURE_END"]) and not events["BOUNDED_STOP"]
    done={row[1]:row for row in events["DONE"]}
    ensure(len(done) == len(events["DONE"]), "Duplicate DONE")
    submissions=[]; previous=[0]*5
    for cycle,sid,base,size in events["SUBMIT"]:
        info=metadata.get("submission_labels",{}).get(str(sid),{})
        sub=dict(id=sid,base=base,bytes=size,start=cycle-origin,end=None,frame_id=info.get("frame_id"),name=info.get("name",f"CB {sid}"))
        if sid in done:
            row=done[sid]; sub["end"]=row[0]-origin;sub["quad_counts"]=dict(zip(("allocated","published","retired","cache_committed","empty_skipped"),[a-b for a,b in zip(row[2:],previous)]));previous=row[2:]
        else: complete=False
        submissions.append(sub)
    partial=not complete
    if partial and not allow_partial: raise ValueError("Incomplete capture; use --allow-partial for diagnostics")
    starts=[s["start"]+origin for s in submissions]
    def sid_at(cycle):
        index=bisect_right(starts,cycle)-1
        if index<0: raise ValueError("GPU event before submission")
        return submissions[index]["id"]
    commands = []
    for end, pc, start, execute, state, word in events["CMD_END"]:
        commands.append(dict(id=len(commands)+1, submission=sid_at(start), pc=pc, start=start - origin, execute=execute - origin,
                             end=end - origin, word=f"{word:08x}", opcode=word >> 28,
                             name=OP[word >> 28], states=[], waits=[]))
    # In-progress views are explicitly marked and never pass final acceptance.
    if partial:
        seen = {(c["submission"],c["pc"]) for c in commands}
        for cycle, pc, word in events["CMD_EXEC"]:
            if (sid_at(cycle),pc) not in seen:
                commands.append(dict(id=len(commands)+1, submission=sid_at(cycle), pc=pc, start=cycle - origin, execute=cycle - origin,
                                     end=stop - origin, word=f"{word:08x}", opcode=word >> 28,
                                     name=OP[word >> 28], states=[], waits=[], partial=True))
    commands.sort(key=lambda c:(c["start"],c["pc"]))
    for index,c in enumerate(commands,1): c["id"]=index
    by_pc = {(c["submission"],c["pc"]): c for c in commands}
    if len(by_pc)!=len(commands): raise ValueError("Repeated command PC within a submission")
    jobs = {}
    for cycle, job, pc, mesh, slot, address, size in events["DMA_JOB"]:
        jobs[job] = dict(id=job, pc=pc, submission=sid_at(cycle), command=by_pc[(sid_at(cycle),pc)]["id"], mesh=mesh, slot=slot, source=address, size=size,
                         start=cycle - origin, end=None, transactions=[])
    for cycle, job in events["DMA_DONE"]:
        ensure(job in jobs and jobs[job]['end'] is None, "Invalid capture: job in jobs and jobs[job]['end'] is None")
        jobs[job]["end"] = cycle - origin
    transactions = {}
    for cycle, tx, job, address, count in events["DMA_REQUEST"]:
        transactions[tx] = dict(id=tx, job=job, submission=sid_at(cycle), start=cycle - origin, end=None,
                                address=address, bytes=32 * (count + 1))
    for cycle, tx, job in events["DMA_RETURN"]:
        ensure(tx in transactions and transactions[tx]['job'] == job, "Invalid capture: tx in transactions and transactions[tx]['job'] == job")
        transactions[tx]["end"] = cycle - origin
    for tx in transactions.values():
        if tx["job"]:
            jobs[tx["job"]]["transactions"].append(tx)
    meshlets = {}
    for cycle, mid, pc, slot, dma in events["DRAW"]:
        meshlets[mid] = dict(id=mid, pc=pc, submission=sid_at(cycle), command=by_pc[(sid_at(cycle),pc)]["id"], slot=slot, dma=dma, start=cycle - origin,
                             vt_end=None, name=metadata.get("meshlet_names",{}).get(str(mid),f"Meshlet {mid}"),
                             vt=[], r=[], i=[], p=[], prepare=[], quads=0, triangles=0,
                             final=[], rop=[], last_quad=None)
        by_pc[(sid_at(cycle),pc)]["meshlet"] = mid
    for cycle, mid in events["VT_END"]:
        meshlets[mid]["vt_end"] = cycle - origin
    for _, mid, _ in events["T_PUBLISH"]:
        meshlets[mid]["triangles"] += 1
    quads = {}
    for cycle, qid, mid, slot in events["Q_ALLOC"]:
        ensure(mid in meshlets and qid not in quads, 'Invalid capture: mid in meshlets and qid not in quads')
        quads[qid] = dict(id=qid, meshlet=mid, start=cycle - origin, slot=slot)
        meshlets[mid]["quads"] += 1
    for cycle, qid, mid, mask in events["Q_PUBLISH"]:
        q = quads[qid]
        ensure(q['meshlet'] == mid and 'publish' not in q, "Invalid capture: q['meshlet'] == mid and 'publish' not in q")
        q.update(publish=cycle - origin, mask=mask)
    for cycle, qid, mid, empty in events["Q_RETIRE"]:
        q = quads[qid]
        ensure(q['meshlet'] == mid and 'retire' not in q, "Invalid capture: q['meshlet'] == mid and 'retire' not in q")
        q.update(retire=cycle - origin, empty=empty)
    for cycle, qid, mid in events["ROP_BEGIN"]:
        q = quads[qid]
        ensure(q['meshlet'] == mid and 'rop' not in q, "Invalid capture: q['meshlet'] == mid and 'rop' not in q")
        q["rop"] = cycle - origin
    for cycle, qid, mid in events["Q_COMMIT"]:
        q = quads[qid]
        ensure(q['meshlet'] == mid and 'commit' not in q, "Invalid capture: q['meshlet'] == mid and 'commit' not in q")
        q["commit"] = cycle - origin
    for cycle, qid, mid, lane in events["FINAL"]:
        q = quads[qid]
        ensure(q['meshlet'] == mid, "Invalid capture: q['meshlet'] == mid")
        q.setdefault("final", cycle - origin)
    for q in quads.values():
        m = meshlets[q["meshlet"]]
        end = q.get("commit", q.get("retire", stop - origin))
        if not partial:
            ensure(q['start'] < q['publish'] < q['retire'] <= end, "Invalid capture: q['start'] < q['publish'] < q['retire'] <= end")
            ensure((q['mask'] == 0) == bool(q['empty']), "Invalid capture: (q['mask'] == 0) == bool(q['empty'])")
            ensure(('commit' in q) != bool(q['empty']), "Invalid capture: ('commit' in q) != bool(q['empty'])")
            if not q["empty"]:
                ensure(q['publish'] <= q['rop'] < q['commit'], "Invalid capture: q['publish'] <= q['rop'] < q['commit']")
        if "publish" in q:
            m["p"].append([q["publish"], end])
        if "final" in q:
            m["final"].append([q["final"], q.get("retire", end)])
        if "rop" in q:
            m["rop"].append([q["rop"], end])
        if m["last_quad"] is None or end > m["last_quad"]["end"]:
            m["last_quad"] = dict(id=q["id"], end=end, empty=q.get("empty", False),
                                   pool_retire=q.get("retire"), cache_commit=q.get("commit"))
    tracks = {key: [] for key, *_ in METRICS}
    # RLE retains exact queue/busy samples; rate tracks store only nonzero edges.
    previous = {}
    runs = {"vt": None, "r": None, "i": None, "prepare": None}
    integrals = defaultdict(int)
    maxima = defaultdict(int)
    first, last = None, None
    with (folder / "load.csv").open() as f:
        for raw in csv.DictReader(f):
            if raw["cycle"] is None or None in raw.values():
                if partial:
                    break
                raise AssertionError("Truncated load record")
            row = {k: int(v) for k, v in raw.items()}
            t = row["cycle"] - origin
            first = t if first is None else first
            if last is not None:
                ensure(t == last + 1, 'Invalid capture: t == last + 1')
            last = t
            row["light_busy"] = int(row["light_tokens"] > 0)
            row["sample_busy"] = int(row["sample_quads"] > 0)
            for key, _, kind, cap in METRICS:
                val = row[key]
                ensure(val >= 0, (key, val, t))
                if cap:
                    ensure(val <= cap, (key, val, cap, t))
                if kind == "rate":
                    if val:
                        tracks[key].append([t, val])
                elif previous.get(key) != val:
                    tracks[key].append([t, val])
                    previous[key] = val
                integrals[key] += val
                maxima[key] = max(maxima[key], val)
            owners = {"vt": (row["vt_owner"], row["vt_kind"]) if row["vt_owned"] else (0, 0),
                      "r": (row["r_owner"], 1) if row["r_busy"] else (0, 0),
                      "i": (row["i_owner"], 1) if row["i_busy"] else (0, 0),
                      "prepare": (row["prepare_owner"], 1) if row["prepare_busy"] else (0, 0)}
            for key, owner in owners.items():
                active = runs[key]
                if active and active[2:] != list(owner):
                    start, _, mid, phase = active
                    meshlets[mid][key].append([start, t, phase] if key == "vt" else [start, t])
                    runs[key] = None
                if owner[0] and runs[key] is None:
                    ensure(owner[0] in meshlets, (key, owner))
                    runs[key] = [t, t + 1, *owner]
    ensure(first is not None, 'Invalid capture: first is not None')
    for key, active in runs.items():
        if active:
            start, _, mid, phase = active
            meshlets[mid][key].append([start, last + 1, phase] if key == "vt" else [start, last + 1])
    for m in meshlets.values():
        for key in ("r", "i", "p", "prepare", "final", "rop"):
            m[key] = union(m[key])
        m["stats"] = {}
        for key in ("r", "i", "p", "prepare", "final", "rop"):
            spans = m[key]
            m["stats"][key] = dict(active=sum(b - a for a, b in spans),
                                    first=spans[0][0] if spans else None,
                                    last=spans[-1][1] if spans else None)
        m["stats"]["vt"] = {VT_KIND[k]: sum(b - a for a, b, kind in m["vt"] if kind == k) for k in VT_KIND}
        if not partial:
            ensure(m['vt_end'] is not None and m['dma'] in jobs, "Invalid capture: m['vt_end'] is not None and m['dma'] in jobs")
            ensure(jobs[m['dma']]['mesh'] and jobs[m['dma']]['end'] <= m['start'], "Invalid capture: jobs[m['dma']]['mesh'] and jobs[m['dma']]['end'] <= m['start']")
    # State observations reveal actual predicates, including maintenance tails.
    state_rows = events["STATE"]
    for index, state in enumerate(state_rows):
        cycle, pc, mode, render_idle, work_busy, dma_idle, memory_busy, flush_complete, waits_done, ce = state
        end = state_rows[index + 1][0] if index + 1 < len(state_rows) else stop
        key=(sid_at(cycle),pc)
        if key not in by_pc or end <= cycle:
            continue
        c = by_pc[key]
        a, b = max(cycle - origin, c["start"]), min(end - origin, c["end"])
        if b <= a:
            continue
        c["states"].append([a, b, mode])
        if c["opcode"] == 4 and mode == 7:
            mask = int(c["word"], 16) & 3
            if mask & 1 and work_busy:
                c["waits"].append([a, b, "DMA"])
            if mask & 2 and not render_idle:
                c["waits"].append([a, b, "Render / cache drain"])
        elif c["opcode"] == 7 and mode == 8 and not flush_complete:
            c["waits"].append([a, b, "Dirty framebuffer flush"])
        elif c["opcode"] == 15 and mode == 9:
            if not render_idle or not dma_idle or memory_busy:
                c["waits"].append([a, b, "Render / DMA / memory drain"])
    for c in commands:
        c["jobs"] = [j["id"] for j in jobs.values() if j["command"] == c["id"]]
        c["dependencies"] = []
        for kind in {w[2] for w in c["waits"]}:
            if kind == "DMA":
                for j in jobs.values():
                    if j["start"] < c["end"] and j["end"] is not None and j["end"] > c["execute"]:
                        c["dependencies"].append(dict(pc=j["pc"], command=j["command"], time=j["end"], kind="DMA LAST"))
            elif kind.startswith("Render"):
                for m in meshlets.values():
                    q = m["last_quad"]
                    if m["start"] < c["end"] and q and c["execute"] < q["end"] <= c["end"]:
                        c["dependencies"].append(dict(pc=m["pc"], command=m["command"], time=q["end"], kind="Last quad / cache commit"))
        if c["opcode"] == 4 and not c["dependencies"]:
            c["dependencies"].append(dict(pc=None, command=None, time=c["end"] - 1, kind="Predicate already satisfied / control edge"))
    for span in cpu:
        span["start"]-=origin;span["end"]-=origin
    if cpu_open:
        for span in cpu_open.values():
            span["start"]-=origin;span["end"]=stop-origin;span["partial"]=True;cpu.append(span)
    if not partial:
        ensure(all((j['end'] is not None for j in jobs.values())), "Invalid capture: all((j['end'] is not None for j in jobs.values()))")
        ensure(all((t['end'] is not None for t in transactions.values())), "Invalid capture: all((t['end'] is not None for t in transactions.values()))")
        ensure([e[1] for e in events['Q_RETIRE']] == list(range(1, len(quads) + 1)), "Invalid capture: [e[1] for e in events['Q_RETIRE']] == list(range(1, len(quads) + 1))")
        ensure([e[1] for e in events['Q_COMMIT']] == sorted((e[1] for e in events['Q_COMMIT'])), "Invalid capture: [e[1] for e in events['Q_COMMIT']] == sorted((e[1] for e in events['Q_COMMIT']))")
        summary=events["DONE"][-1][2:]
        ensure(summary == [len(quads), len(quads), len(quads), sum(('commit' in q for q in quads.values())), sum((q.get('empty', 0) for q in quads.values()))], "Invalid capture: summary == [len(quads), len(quads), len(quads), sum(('commit' in q for q in quads.values())), sum((q.get('empty', 0) for q in quads.values()))]")
    data=dict(schema=SCHEMA,version=VERSION,partial=partial,clock_hz=metadata["clock_hz"],origin=origin,
        title=metadata.get("title","GPU profiling session"),start=0,end=max(stop-origin,last+1,max((s["end"] for s in cpu),default=0)),
        submit=submissions[0]["start"],gpu_end=max((s["end"] or stop-origin for s in submissions)),
        submissions=submissions,commands=commands,meshlets=list(meshlets.values()),jobs=list(jobs.values()),
        dma_transactions=list(transactions.values()),cpu=cpu,metric_defs=[dict(key=k,label=l,kind=t,capacity=c,peak=maxima[k],integral=integrals[k]) for k,l,t,c in METRICS],
        tracks=tracks,sample_start=first,sample_end=last+1,provenance=metadata,
        checks=dict(complete_capture=not partial,quad_conservation=not partial))
    validate(data)
    return data


LEGACY_CPU = {101: "DS2 snapshot / timer", 102: "Telemetry", 103: "LED / markers",
              1: "Buttons / frame stats", 2: "Camera / light", 3: "Begin CB / pass",
              4: "Global state", 5: "Box material", 6: "Record box", 7: "Record RGB",
              8: "Rabbit material", 9: "Record rabbit", 10: "Record UI", 11: "Finish CB",
              12: "Submit / cache barrier", 13: "GPU wait / completion"}


def load_profile(path):
    path = Path(path)
    data = json.loads(path.read_text(encoding="utf-8"))
    if data.get("version") == 1 and "schema" not in data:
        data = copy.deepcopy(data)
        data.update(schema=SCHEMA, version=VERSION, title="Imported GPU capture")
        data["provenance"] = {"imported_v1_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                              "legacy_checks": data.get("checks", {})}
        data["submissions"] = [dict(id=1, base=None,
            bytes=max((c["pc"] for c in data["commands"]), default=-4) + 4,
            start=data["submit"], end=None if data["partial"] else data["gpu_end"],
            frame_id=1, name="CB 1")]
        mapping = {c["pc"]: i for i, c in enumerate(data["commands"], 1)}
        for c in data["commands"]:
            c.update(id=mapping[c["pc"]], submission=1)
            for dep in c["dependencies"]:
                dep["command"] = mapping.get(dep["pc"])
        for item in [*data["meshlets"], *data["jobs"]]:
            item.update(command=mapping[item["pc"]], submission=1)
        for tx in data["dma_transactions"]:
            tx["submission"] = 1
        for i, span in enumerate(data["cpu"], 1):
            span.update(name=LEGACY_CPU.get(span["id"], "CPU stage " + str(span["id"])),
                        stage_id=span["id"], id=i, frame_id=1)
        data["provenance"]["legacy_session_end"] = data["end"]
        data["end"] = max(data["end"], max((s["end"] for s in data["cpu"]), default=0))
    return validate(data)


def write_report(data, folder):
    validate(data)
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(data, separators=(",", ":"), ensure_ascii=True)
    (folder / "profile.json").write_text(payload + "\n", encoding="utf-8")
    template = Path(__file__).with_name("viewer.html").read_text(encoding="utf-8")
    safe = payload.replace("<", "\\u003c").replace(">", "\\u003e").replace("&", "\\u0026")
    (folder / "profiler.html").write_text(template.replace("/*PROFILE_DATA*/null", safe), encoding="utf-8")
    (folder / "validation.json").write_text(json.dumps(data["checks"], indent=2) + "\n", encoding="utf-8")
    meshes = {m["id"]: m for m in data["meshlets"]}
    jobs = {j["id"]: j for j in data["jobs"]}
    def async_end(command):
        ends = [command["end"], *[jobs[j]["end"] or data["end"] for j in command["jobs"]]]
        mesh = meshes.get(command.get("meshlet"))
        if mesh and mesh["last_quad"]:
            ends.append(mesh["last_quad"]["end"])
        return max(ends)

    for name, fields, rows in [
        ("submissions", ["id", "frame_id", "name", "base", "bytes", "start", "end"], data["submissions"]),
        ("commands", ["id", "submission", "pc", "opcode", "name", "word", "start", "execute", "end", "async_end"],
         [{**c, "async_end": async_end(c)} for c in data["commands"]]),
        ("meshlets", ["id", "submission", "command", "pc", "name", "start", "vt_end", "quads", "triangles"], data["meshlets"]),
        ("cpu", ["id", "frame_id", "name", "start", "end"], data["cpu"]),
    ]:
        with (folder / (name + ".csv")).open("w", newline="", encoding="utf-8") as stream:
            writer = csv.DictWriter(stream, fields, extrasaction="ignore")
            writer.writeheader(); writer.writerows(rows)


def validate(data):
    def require(condition, message):
        if not condition:
            raise ValueError(message)

    def integer(value, label, minimum=0):
        require(type(value) is int and minimum <= value <= MAX_INTEGER, label)

    require(data.get("schema") == SCHEMA and data.get("version") == VERSION,
            "Unsupported GPU profile schema/version")
    integer(data.get("clock_hz"), "clock_hz must be a positive integer", 1)
    integer(data.get("start"), "Invalid session start")
    integer(data.get("end"), "Invalid session end")
    require(data["end"] > data["start"], "Empty session")
    require(type(data.get("partial")) is bool, "Missing partial status")

    def interval(a, b, label):
        integer(a, label); integer(b, label)
        require(data["start"] <= a <= b <= data["end"], label)

    def index(items, label):
        result = {}
        for item in items:
            integer(item.get("id"), label + " ID", 1)
            require(item["id"] not in result, "Duplicate " + label + " ID")
            result[item["id"]] = item
        return result

    submissions = index(data["submissions"], "submission")
    require(bool(submissions), "Session needs an accepted submission")
    commands = index(data["commands"], "command")
    meshes = index(data["meshlets"], "meshlet")
    jobs = index(data["jobs"], "DMA job")
    index(data["dma_transactions"], "DMA transaction")
    index(data["cpu"], "CPU span")
    previous = data["start"]
    previous_id = 0
    for sub in data["submissions"]:
        require(sub["id"] > previous_id, "Submission IDs must increase")
        previous_id = sub["id"]
        require(sub["start"] >= previous, "GPU submissions overlap or are unordered")
        require(sub["end"] is not None or data["partial"], "Unfinished submission")
        interval(sub["start"], sub["end"] or data["end"], "Submission bounds")
        previous = sub["end"] or data["end"]
        if sub.get("base") is not None:
            integer(sub["base"], "CB byte base")
            require(sub["base"] % 128 == 0, "Unaligned CB base")
        if sub.get("bytes") is not None:
            require(0 < sub["bytes"] <= 65536 and sub["bytes"] % 4 == 0, "CB length")
            if not data["partial"]:
                pcs = [c["pc"] for c in data["commands"] if c["submission"] == sub["id"]]
                require(pcs == list(range(0, sub["bytes"], 4)), "Missing/reordered command occurrences")
        counts = sub.get("quad_counts")
        if counts:
            require(counts["allocated"] == counts["published"] == counts["retired"] ==
                    counts["cache_committed"] + counts["empty_skipped"], "Quad conservation")
    seen = set()
    for c in commands.values():
        require(c["submission"] in submissions, "Unknown command submission")
        integer(c["pc"], "Command PC")
        require(c["pc"] % 4 == 0, "Unaligned command PC")
        key = (c["submission"], c["pc"])
        require(key not in seen, "Duplicate command PC within submission")
        seen.add(key)
        require(type(c["opcode"]) is int and 0 <= c["opcode"] < 16, "Opcode")
        require(isinstance(c["word"], str) and len(c["word"]) == 8 and
                int(c["word"], 16) >> 28 == c["opcode"], "Instruction word/opcode")
        interval(c["start"], c["execute"], "Command fetch/execute bounds")
        interval(c["execute"], c["end"], "Command execute/release bounds")
        sub = submissions[c["submission"]]
        require(sub["start"] <= c["start"] and c["end"] <= (sub["end"] or data["end"]),
                "Command outside its submission")
        if c.get("meshlet") is not None:
            require(c["meshlet"] in meshes and meshes[c["meshlet"]]["command"] == c["id"],
                    "Meshlet binding")
        for jid in c["jobs"]:
            require(jid in jobs and jobs[jid]["command"] == c["id"], "DMA command binding")
        for dep in c["dependencies"]:
            require(dep["command"] is None or dep["command"] in commands, "Dependency command")
            interval(dep["time"], dep["time"], "Dependency time")
    for item in [*jobs.values(), *meshes.values()]:
        require(item["command"] in commands, "Unknown owner command")
        require(item["submission"] == commands[item["command"]]["submission"], "Owner submission")
    for job in jobs.values():
        require(job["end"] is not None or data["partial"], "Unfinished DMA job")
        interval(job["start"], job["end"] or data["end"], "DMA bounds")
        sub = submissions[job["submission"]]
        require(sub["start"] <= job["start"] and (job["end"] or data["end"]) <=
                (sub["end"] or data["end"]), "DMA outside submission")
    for mesh in meshes.values():
        require(mesh["dma"] in jobs, "Meshlet DMA job")
        for key in ("vt", "r", "i", "p", "prepare", "final", "rop"):
            previous = data["start"]
            for a, b, *_ in mesh[key]:
                interval(a, b, "Meshlet stage bounds")
                require(a >= previous, "Unordered/overlapping stage intervals")
                previous = b
        q = mesh["last_quad"]
        if q:
            interval(q["pool_retire"] or q["end"], q["end"], "Last quad retirement")
            sub = submissions[mesh["submission"]]
            require(q["end"] <= (sub["end"] or data["end"]), "Meshlet survives submission completion")
    for tx in data["dma_transactions"]:
        require(tx["submission"] in submissions, "Transaction submission")
        require(tx["job"] == 0 or tx["job"] in jobs, "Transaction DMA owner")
        if tx["job"]:
            require(tx["submission"] == jobs[tx["job"]]["submission"], "Transaction crosses submissions")
        require(tx["end"] is not None or data["partial"], "Unfinished DMA transaction")
        interval(tx["start"], tx["end"] or data["end"], "Transaction bounds")
    for span in data["cpu"]:
        interval(span["start"], span["end"], "CPU span bounds")
    interval(data["sample_start"], data["sample_end"], "Sample bounds")
    require({"vt_owned", "r_busy", "i_busy", "quad_total", "rop_contexts"} <= data["tracks"].keys(),
            "Missing overview metrics")
    for metric in data["metric_defs"]:
        key, kind = metric["key"], metric["kind"]
        require(kind in ("binary", "queue", "rate") and key in data["tracks"], "Metric definition")
        previous = -1
        for t, v in data["tracks"][key]:
            interval(t, t, "Metric time")
            require(t > previous, "Metric times must increase")
            integer(v, "Metric value")
            if kind == "binary":
                require(v <= 1, "Binary metric")
            if metric["capacity"]:
                require(v <= metric["capacity"], "Metric capacity")
            previous = t
    return data
