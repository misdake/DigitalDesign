"""Export camera samples in the recording's world coordinates."""

import argparse
import gzip
import json
from pathlib import Path


def camera_samples(path, start, step, count):
    open_trace = gzip.open if path.suffix == '.gz' else open
    with open_trace(path, 'rt', encoding='utf-8') as stream:
        header = json.loads(next(stream))
        if header['format'] != 'tankracer-recording' or header['version'] != 2:
            raise ValueError('expected a TRR2 recording')
        fields = header['fields']
        position = [fields.index(f'camera_position[{i}]') for i in range(3)]
        matrix = [fields.index(f'camera_matrix[{i}]') for i in range(9)]
        watched = set(position + matrix)
        state = {i: header['initial'][i] for i in watched}
        stop = start + (count - 1) * step
        for line in stream:
            row = json.loads(line)
            sequence = row['sequence']
            if sequence > stop:
                break
            changes = iter(row['changes'])
            for index, value in zip(changes, changes):
                if index in watched:
                    state[index] = value
            if sequence >= start and (sequence - start) % step == 0:
                eye = tuple(state[i] / 1024 for i in position)
                forward = tuple(state[i] / 4096 for i in matrix[6:])
                up = tuple(state[i] / 4096 for i in matrix[3:6])
                yield dict(eye=eye, target=[p + d for p, d in zip(eye, forward)],
                           up=up, label=f'recorded-{sequence}')


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('recording', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--start', type=int, required=True)
    parser.add_argument('--step', type=int, default=2)
    parser.add_argument('--count', type=int, default=64)
    args = parser.parse_args()
    if args.start < 1 or args.step < 1 or args.count < 1:
        parser.error('start, step, and count must be positive')
    samples = list(camera_samples(args.recording, args.start, args.step, args.count))
    if len(samples) != args.count:
        parser.error(f'recording yielded {len(samples)} of {args.count} requested samples')
    args.output.write_text(json.dumps(samples, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
