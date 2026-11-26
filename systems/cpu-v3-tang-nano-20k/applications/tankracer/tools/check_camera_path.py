"""Bounded camera interpolation and production playback-clock checks."""
import json
import math
from pathlib import Path
import subprocess
import tempfile
import unittest

from camera_path import expand, recorded_lap

ROOT = Path(__file__).resolve().parents[1]


class CameraPath(unittest.TestCase):
    def test_legacy_sequence_is_unchanged(self):
        cameras = json.loads((ROOT / 'assets/england/cameras.json').read_text())
        self.assertEqual(expand(cameras), (cameras, 0, 0))

    def test_linear_distance_and_closed_seam(self):
        corners = [[0, 2, 0], [4, 2, 0], [4, 2, 4], [0, 2, 4]]
        value = dict(format='tankracer.camera-loop.v1', duration_seconds=1,
                     frames_per_second=8, views=[dict(eye=p, target=[p[0], 2, p[2]+1])
                                                 for p in corners])
        cameras, fps, duration = expand(value)
        self.assertEqual((fps, duration), (8, 1000))
        self.assertEqual([c['eye'] for c in cameras],
                         [[0, 2, 0], [2, 2, 0], [4, 2, 0], [4, 2, 2],
                          [4, 2, 4], [2, 2, 4], [0, 2, 4], [0, 2, 2]])
        self.assertTrue(all(math.dist(a['eye'], b['eye']) == 2
                            for a, b in zip(cameras, cameras[1:] + cameras[:1])))

    def test_recorded_lap_is_reproducible_and_continuous(self):
        assets = ROOT / 'assets/england'
        loop = recorded_lap(json.loads((assets/'cameras.json').read_text()),
                            json.loads((assets/'route.json').read_text()))
        self.assertEqual(loop, json.loads((assets/'cameras-loop.json').read_text()))
        cameras, fps, duration = expand(loop)
        self.assertEqual((len(cameras), fps, duration), (1800, 30, 60000))
        for a, b in zip(cameras, cameras[1:] + cameras[:1]):
            self.assertLess(math.dist(a['eye'], b['eye']), 1.3)
            forward = [y-x for x, y in zip(a['eye'], a['target'])]
            self.assertAlmostEqual(math.hypot(*forward), 1)
            self.assertEqual(a['up'], [0, 1, 0])

    def test_bad_loop_is_rejected(self):
        loop = json.loads((ROOT/'assets/england/cameras-loop.json').read_text())
        for field, value in [('duration_seconds', 0), ('duration_seconds', 61),
                             ('frames_per_second', 61), ('frames_per_second', 60)]:
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                expand(dict(loop, **{field: value}))
        with self.assertRaisesRegex(ValueError, 'zero-length'):
            expand(dict(loop, views=[loop['views'][0]]*3))

    def test_actual_playback_body_handles_timer_wrap_and_skipped_frames(self):
        source = (ROOT/'rcc/playback.rs').read_text()
        body = source[source.index('pub struct Playback'):]
        temporary = ROOT.parents[3]/'target/tmp/tankracer-camera-playback'
        temporary.mkdir(parents=True, exist_ok=True)
        for fps, duration, count in [(30, 60000, 1800), (60, 1000, 60), (0, 0, 118)]:
            program = f'''
use std::sync::atomic::{{AtomicU16, Ordering}};
static CLOCK: AtomicU16 = AtomicU16::new(65000);
const CAMERA_FPS:u16={fps}; const CAMERA_LOOP_MS:u16={duration};
const CAMERA_COUNT:u16={count};
fn time_milliseconds()->u16 {{ CLOCK.load(Ordering::Relaxed) }}
fn time_elapsed_ms(a:u16,b:u16)->u16 {{ b.wrapping_sub(a) }}
{body}
fn main() {{
    let mut playback=Playback::new();
    for step in 0..15000u32 {{
        let elapsed=step*17;
        CLOCK.store((65000+elapsed) as u16, Ordering::Relaxed);
        let expected=if CAMERA_FPS==0 {{ (step % CAMERA_COUNT as u32) as u16 }}
                     else {{ ((elapsed % {max(1, duration)}) * CAMERA_FPS as u32 / 1000) as u16 }};
        assert_eq!(playback.next(), expected, "step {{step}}");
    }}
    if CAMERA_FPS!=0 {{
        let elapsed=14999*17+2500;
        CLOCK.store((65000+elapsed) as u16, Ordering::Relaxed);
        assert_eq!(playback.next(), ((elapsed % {max(1, duration)})*CAMERA_FPS as u32/1000) as u16);
    }}
}}
'''
            with self.subTest(fps=fps, duration=duration), tempfile.TemporaryDirectory(dir=temporary) as folder:
                path = Path(folder)/'clock.rs'
                path.write_text(program)
                executable = Path(folder)/'clock.exe'
                subprocess.run(['rustc', '--edition=2021', '-C', 'overflow-checks=off',
                                str(path), '-o', str(executable)], check=True, timeout=60)
                subprocess.run([str(executable)], check=True, timeout=10)


if __name__ == '__main__':
    unittest.main()
