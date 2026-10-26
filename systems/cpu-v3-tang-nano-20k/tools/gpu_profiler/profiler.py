"""Capture, correlate or render versioned GPU profiling sessions (stdlib only)."""
import argparse
import json
from pathlib import Path
import sys
import subprocess
import tempfile

sys.dont_write_bytecode = True
from model import build_capture, load_profile, write_report


def main():
    parser = argparse.ArgumentParser(__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    capture = commands.add_parser("capture", help="Bounded read-only RTL capture from a manifest")
    capture.add_argument("manifest", type=Path)
    capture.add_argument("--out", type=Path, required=True)
    capture.add_argument("--compile-only", action="store_true")
    build = commands.add_parser("build", help="Correlate observer events/load into a report")
    build.add_argument("folder", type=Path)
    build.add_argument("--out", type=Path)
    build.add_argument("--allow-partial", action="store_true")
    render = commands.add_parser("render", help="Validate v2 or import legacy v1 JSON, then render")
    render.add_argument("profile", type=Path)
    render.add_argument("--out", type=Path, required=True)
    check = commands.add_parser("check", help="Bounded format, statistics and viewer regression")
    check.add_argument("--node", default="node")
    args = parser.parse_args()
    if args.command == "check":
        root = Path(__file__).resolve().parent
        subprocess.run([sys.executable, "-m", "unittest", "discover", "-s", str(root / "tests"),
                        "-p", "test_profiler.py", "-v"], check=True, timeout=30)
        with tempfile.TemporaryDirectory() as temp:
            write_report(build_capture(root / "tests/fixtures/pingpong"), temp)
            for script, extra in [("math_checks.js", []), ("viewer_checks.js", [str(Path(temp) / "profile.json")])]:
                subprocess.run([args.node, str(root / "tests" / script), *extra], check=True, timeout=20)
    elif args.command == "capture":
        from capture import run
        run(args.manifest, args.out, args.compile_only)
    else:
        data = (build_capture(args.folder, args.allow_partial) if args.command == "build"
                else load_profile(args.profile))
        out = args.out or args.folder
        write_report(data, out)
        print(json.dumps({"out": str(out.resolve()), "submissions": len(data["submissions"]),
                          "commands": len(data["commands"]), "partial": data["partial"]}))


if __name__ == "__main__":
    main()
