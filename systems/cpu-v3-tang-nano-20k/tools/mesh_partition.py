"""Official pinned meshoptimizer partitioning for source-geometry producers."""
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent
SOURCES = [Path(__file__), ROOT/'build_meshlets.mjs', ROOT/'vendor/meshoptimizer/meshopt_clusterizer.mjs',
           ROOT/'vendor/meshoptimizer/UPSTREAM.md', ROOT/'vendor/meshoptimizer/LICENSE.md']


def partition(positions, triangles):
    request = dict(positions=[v for p in positions for v in p], indices=[i for t in triangles for i in t])
    try:
        result = subprocess.run([os.environ.get('NODE_EXE', 'node'), str(ROOT/'build_meshlets.mjs')],
                                input=json.dumps(request), capture_output=True, text=True, timeout=60,
                                creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
    except FileNotFoundError as error:
        raise RuntimeError('Meshlet generation requires Node.js 18+ on PATH (or NODE_EXE). No npm install is needed.') from error
    except subprocess.TimeoutExpired as error:
        raise RuntimeError('meshoptimizer exceeded the 60-second partition budget') from error
    if result.returncode:
        raise RuntimeError('meshoptimizer partition failed: ' + result.stderr[-2048:])
    parts = json.loads(result.stdout)
    for part in parts:
        vertices, faces = part['vertices'], part['triangles']
        if (not 0 < len(vertices) <= 64 or len(set(vertices)) != len(vertices)
                or any(type(i) is not int or not 0 <= i < len(positions) for i in vertices)
                or not 0 < len(faces) <= 96
                or any(len(t) != 3 or any(type(i) is not int or not 0 <= i < len(vertices) for i in t) for t in faces)):
            raise ValueError('meshoptimizer returned invalid native meshlet references')
    if not parts or sum(len(p['triangles']) for p in parts) != len(triangles):
        raise ValueError('meshoptimizer changed the triangle count')
    return [(p['vertices'], p['triangles']) for p in parts]
