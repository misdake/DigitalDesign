"""Run an application producer with the system SDK and application imports."""
from pathlib import Path
import runpy
import sys


def main():
    if len(sys.argv) < 2:
        raise SystemExit('usage: asset_producer.py SCRIPT [producer arguments]')
    script = Path(sys.argv[1]).resolve(strict=True)
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(script.parent))
    sys.argv = [str(script), *sys.argv[2:]]
    runpy.run_path(str(script), run_name='__main__')


if __name__ == '__main__':
    main()
