"""Bake a constant-speed, level-horizon lap from recorded driving cameras."""
import argparse
import bisect
import json
import math
from pathlib import Path

MAX_CAMERAS = 2730  # A 24-word view must fit the asset window's u16 word offset.


def vector(value):
    if not isinstance(value, (list, tuple)) or len(value) != 3 or any(
            type(x) not in (int, float) or not math.isfinite(x) for x in value):
        raise ValueError('camera vectors need three finite numbers')
    return list(value)


def unit(value):
    length = math.hypot(*value)
    if length < 1e-8:
        raise ValueError('degenerate camera direction')
    return [x / length for x in value]


def mix(a, b, fraction):
    return [x + (y - x) * fraction for x, y in zip(a, b)]


def pose(camera):
    eye, target = vector(camera['eye']), vector(camera['target'])
    return dict(eye=eye, forward=unit([b - a for a, b in zip(eye, target)]))


def interpolate(a, b, fraction):
    eye = mix(a['eye'], b['eye'], fraction)
    forward = unit(mix(a['forward'], b['forward'], fraction))
    if math.hypot(forward[0], forward[2]) < 1e-6:
        raise ValueError('camera direction is parallel to the level horizon up vector')
    return dict(eye=eye, target=[x + y for x, y in zip(eye, forward)], up=[0, 1, 0])


def expand(value):
    """Legacy lists advance per rendered frame; loop descriptors use wall time."""
    if isinstance(value, list):
        if not 1 <= len(value) <= MAX_CAMERAS:
            raise ValueError(f'expected 1..{MAX_CAMERAS} camera samples')
        return value, 0, 0
    if not isinstance(value, dict) or value.get('format') != 'tankracer.camera-loop.v1':
        raise ValueError('expected a camera list or tankracer.camera-loop.v1')
    seconds, fps = value['duration_seconds'], value['frames_per_second']
    if type(seconds) is not int or not 1 <= seconds <= 60:
        raise ValueError('camera loop duration must be 1..60 seconds')
    if type(fps) is not int or not 1 <= fps <= 60 or seconds * fps > MAX_CAMERAS:
        raise ValueError('camera loop sample count exceeds the asset window')
    knots = [pose(camera) for camera in value['views']]
    if len(knots) < 3:
        raise ValueError('camera loop needs at least three knots')
    distances = [0.0]
    for a, b in zip(knots, knots[1:] + knots[:1]):
        length = math.dist(a['eye'], b['eye'])
        if length < 1e-8:
            raise ValueError('camera loop has a zero-length segment')
        distances.append(distances[-1] + length)
    frames = []
    count = seconds * fps
    for index in range(count):
        distance = distances[-1] * index / count
        segment = min(len(knots) - 1, bisect.bisect_right(distances, distance) - 1)
        fraction = (distance - distances[segment]) / (distances[segment + 1] - distances[segment])
        camera = interpolate(knots[segment], knots[(segment + 1) % len(knots)], fraction)
        camera['label'] = f'loop-{index:04d}'
        frames.append(camera)
    return frames, fps, seconds * 1000


def recorded_lap(cameras, route):
    """Use the last complete forward crossing pair, avoiding the initial launch."""
    points = {point['name']: point['position'] for point in route['points']}
    start, following = [points[name] for name in route['main_loop'][:2]]
    tangent = unit([following[0] - start[0], 0, following[2] - start[2]])
    poses = [pose(camera) for camera in cameras]
    crossings = []
    for index, (a, b) in enumerate(zip(poses, poses[1:])):
        before = sum((x - y) * z for x, y, z in zip(a['eye'], start, tangent))
        after = sum((x - y) * z for x, y, z in zip(b['eye'], start, tangent))
        if before < 0 <= after:
            fraction = -before / (after - before)
            crossing = interpolate(a, b, fraction)
            if math.hypot(crossing['eye'][0] - start[0], crossing['eye'][2] - start[2]) < 30:
                crossings.append((index, crossing))
    if len(crossings) < 2:
        raise ValueError('recording does not contain a complete lap at the route start')
    (first, a), (last, b) = crossings[-2:]
    # One shared seam pose closes the small difference between recorded laps.
    seam = interpolate(pose(a), pose(b), 0.5)
    views = [seam] + [interpolate(p, p, 0) for p in poses[first + 1:last + 1]]
    return dict(format='tankracer.camera-loop.v1', duration_seconds=60,
                frames_per_second=30, views=views)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('recording', type=Path)
    parser.add_argument('route', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    loop = recorded_lap(json.loads(args.recording.read_text()), json.loads(args.route.read_text()))
    frames, fps, duration = expand(loop)
    args.output.write_text(json.dumps(loop, indent=2) + '\n', encoding='utf-8')
    print(f'{len(loop["views"])} knots, {len(frames)} samples, {fps} Hz, {duration / 1000:g} s/lap')


if __name__ == '__main__':
    main()
