"""Capture, correlate or render versioned GPU profiling sessions (stdlib only)."""
import argparse
import json
from pathlib import Path
import sys
import subprocess
import tempfile

sys.dont_write_bytecode = True
from model import build_capture, write_report
from reporting import build_report, render_report, summary, image_options
from snapshot import SnapshotError


def main():
    parser = argparse.ArgumentParser(__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    capture = commands.add_parser("capture", help="Bounded read-only RTL capture from a manifest")
    capture.add_argument("manifest", type=Path)
    capture.add_argument("--out", type=Path, required=True)
    capture.add_argument("--compile-only", action="store_true")
    image_options(capture)
    build = commands.add_parser("build", help="Correlate observer events/load into a report")
    build.add_argument("folder", type=Path)
    build.add_argument("--out", type=Path)
    build.add_argument("--allow-partial", action="store_true")
    image_options(build, replace=True)
    render = commands.add_parser("render", help="Validate v2 or import legacy v1 JSON, then render")
    render.add_argument("profile", type=Path)
    render.add_argument("--out", type=Path, required=True)
    render.add_argument('--require-images', action='store_true')
    render.add_argument('--replace-report', action='store_true')
    check = commands.add_parser("check", help="Bounded format, statistics and viewer regression")
    check.add_argument("--node", default="node")
    args = parser.parse_args()
    if args.command == "check":
        root = Path(__file__).resolve().parent
        subprocess.run([sys.executable, "-m", "unittest", "discover", "-s", str(root / "tests"),
                        "-p", "test_*.py", "-v"], check=True, timeout=90)
        with tempfile.TemporaryDirectory() as temp:
            write_report(build_capture(root / "tests/fixtures/pingpong"), temp)
            for script, extra in [("math_checks.js", []), ("viewer_checks.js", [str(Path(temp) / "profile.json")]), ("result_viewer_checks.js", [])]:
                subprocess.run([args.node, str(root / "tests" / script), *extra], check=True, timeout=20)
    elif args.command == "capture":
        from capture import run
        run(args.manifest, args.out, args.compile_only)
        if not args.compile_only:
            data = build_report(args.out, args.out/'profile', snapshot=args.snapshot, raw=args.raw,
                binding=args.binding, preview=args.preview, capture_format=args.capture_format,
                require_images=args.require_images)
            print(json.dumps(summary(data, args.out/'profile')))
    else:
        if args.command == 'render':
            raw = json.loads(args.profile.read_text())
            if raw.get('schema') == 'digitaldesign.gpu-result':
                if args.require_images or args.replace_report:
                    raise SnapshotError('Result-only render has no command history or generated profiling report replacement')
                from simulator import write_result
                write_result(raw, args.out, args.profile.parent)
                print(json.dumps(dict(out=str(args.out.resolve()), backend='sim', complete=True)))
                return
        out = args.out or args.folder
        data = (build_report(args.folder, out, snapshot=args.snapshot, raw=args.raw, binding=args.binding,
                            allow_partial=args.allow_partial, preview=args.preview,
                            capture_format=args.capture_format, require_images=args.require_images,
                            replace_report=args.replace_report)
                if args.command == 'build' else render_report(args.profile, out, require_images=args.require_images,
                                                             replace_report=args.replace_report))
        print(json.dumps(summary(data, out)))


if __name__ == "__main__":
    try:
        main()
    except (SnapshotError, OSError, KeyError, TypeError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print('Report refused: '+str(error), file=sys.stderr)
        raise SystemExit(1)
