"""Protocol and interval arithmetic; these are synthetic interface events."""
from collections import defaultdict
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import memory

CLOCK = 54_000_000


def burst(port=0, source=1):
    e = defaultdict(list)
    e['MEM_REQUEST'] = [[0, port, 0x200000, 0, source]]
    e['MEM_DATA'] = [[c, port, 255, 0] for c in range(1, 5)]
    e['MEM_COMPLETE'] = [[5, port, 0]]
    return e


class MemoryTests(unittest.TestCase):
    def test_actual_beats_are_not_request_bytes(self):
        e = burst()
        report = memory.build(e, 0, 512, CLOCK, True, [dict(id=1, start=2, complete_cycle=4)], {})
        s = report['summary']
        self.assertEqual((s['issued_transactions'], s['completed_transactions'], s['data_beats']), (1, 1, 4))
        self.assertEqual(s['wire_bytes'], 32)
        self.assertEqual(s['average_wire_bytes_per_second'], 32 * CLOCK / 512)
        self.assertEqual(s['peak_wire_bytes_per_second'], 32 * CLOCK / 256)
        self.assertEqual(s['links'][0]['utilization'], 32 / (8 * 512))
        selected = report['commands']['1']
        self.assertEqual((selected['issued_transactions'], selected['completed_transactions'], selected['wire_bytes']), (0, 0, 16))
        self.assertIsNone(selected['peak_wire_bytes_per_second'])

    def test_two_queued_requests_keep_source_until_last(self):
        e = burst()
        e['MEM_REQUEST'].append([1, 0, 0x200010, 0, 3])
        e['MEM_DATA'] += [[c, 0, 255, 0] for c in range(5, 9)]
        e['MEM_COMPLETE'].append([9, 0, 0])
        report = memory.build(e, 0, 16, CLOCK, True, [], {})
        sources = {s['name']: s for s in report['summary']['sources']}
        self.assertEqual(sources['command-fetch']['wire_bytes'], 32)
        self.assertEqual(sources['local-dma']['wire_bytes'], 32)
        self.assertEqual(report['summary']['completed_transactions'], 2)

    def test_write_data_and_terminal_ack_are_separate(self):
        e = burst(2, 5)
        e['MEM_COMPLETE'] = [[10, 2, 0]]
        report = memory.build(e, 0, 16, CLOCK, True, [], {'fb_write_wait': [[0, 1], [2, 0]]})
        s = report['summary']
        self.assertEqual(s['write_wire_bytes'], 32)
        self.assertEqual(s['read_wire_bytes'], 0)
        self.assertEqual(s['write_terminal_wait_link_cycles'], 5)
        self.assertEqual(s['request_backpressure_link_cycles'], 2)

    def test_masks_and_failed_read_payload_are_not_useful_bytes(self):
        e = burst()
        e['MEM_DATA'][0][2] = 3
        e['MEM_DATA'][-1][-1] = 1
        e['MEM_COMPLETE'][0][-1] = 1
        s = memory.build(e, 0, 16, CLOCK, True, [], {})['summary']
        self.assertEqual((s['wire_bytes'], s['effective_bytes'], s['failed_transactions']), (32, 18, 1))

    def test_short_last_and_false_complete_are_refused(self):
        e = burst()
        e['MEM_DATA'].pop()
        with self.assertRaisesRegex(ValueError, 'LAST'):
            memory.build(e, 0, 16, CLOCK, True, [], {})
        e = burst()
        e['MEM_COMPLETE'].clear()
        with self.assertRaisesRegex(ValueError, 'outstanding'):
            memory.build(e, 0, 16, CLOCK, True, [], {})
        report = memory.build(e, 0, 16, CLOCK, False, [], {})
        self.assertFalse(report['complete'])
        self.assertEqual(report['summary']['completed_transactions'], 0)

    def test_missing_events_stay_unavailable_and_nested_spans_do_not_subtract(self):
        self.assertFalse(memory.build({}, 0, 16, CLOCK, True, [], {})['available'])
        self.assertEqual(memory.union_length([(0, 10), (1, 2), (10, 11)]), 11)

    def test_duplicate_slots_and_inconsistent_report_totals_are_refused(self):
        e = burst()
        e['MEM_DATA'].append(e['MEM_DATA'][0][:])
        with self.assertRaisesRegex(ValueError, 'Duplicate'):
            memory.build(e, 0, 16, CLOCK, True, [], {})
        report = memory.build(burst(), 0, 512, CLOCK, True, [], {})
        memory.validate(report, CLOCK, [], 512)
        report['summary']['sources'][0]['issued_transactions'] += 1
        with self.assertRaisesRegex(ValueError, 'conserve'):
            memory.validate(report, CLOCK, [], 512)


if __name__ == '__main__':
    unittest.main()
