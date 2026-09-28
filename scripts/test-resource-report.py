#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Check measurement parsing and fail-closed behavior without invoking profilers."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location('resources', Path(__file__).with_name('measure-resources.py'))
RESOURCES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RESOURCES)


class Reports(unittest.TestCase):
    def test_allocation_summary(self):
        text = '==1== total heap usage: 1,234 allocs, 1,233 frees, 56,789 bytes allocated\n==1== ERROR SUMMARY: 0 errors from 0 contexts\n'
        self.assertEqual(RESOURCES.memcheck(text), {'allocation_calls': 1234, 'free_calls': 1233, 'total_allocated_bytes': 56789})
        for broken in (text.replace('0 errors', '1 errors'), text.split('\n')[0], ''):
            with self.assertRaises(ValueError):
                RESOURCES.memcheck(broken)

    def test_peak_semantics(self):
        text = 'snapshot=0\nmem_heap_B=100\nmem_heap_extra_B=16\nmem_stacks_B=0\nheap_tree=peak\nsnapshot=1\nmem_heap_B=105\nmem_heap_extra_B=0\nmem_stacks_B=0\nheap_tree=empty\n'
        self.assertEqual(RESOURCES.massif(text), {'observed_peak_requested_heap_bytes': 105, 'observed_peak_heap_plus_estimated_overhead_bytes': 116, 'snapshots': 2})
        for broken in ('', text.replace('heap_tree=peak', 'heap_tree=empty'), text.replace('mem_heap_extra_B=16\n', '')):
            with self.assertRaises(ValueError):
                RESOURCES.massif(broken)

    def test_native_rss(self):
        self.assertEqual(RESOURCES.rss(b'ok empty\nrss_bytes 4096\n', 'empty'), 4096)
        for output in (b'ok empty\n', b'ok empty\nrss_bytes 0\n', b'ok empty\nrss_bytes -1\n', b'ok empty\nrss_bytes 4096\nextra'):
            with self.assertRaises(ValueError):
                RESOURCES.rss(output, 'empty')

    def test_workload_output(self):
        RESOURCES.verify_output(b'ok empty\n', 'empty')
        for output in (b'', b'ok another\n', b'ok empty\nextra'):
            with self.assertRaises(ValueError):
                RESOURCES.verify_output(output, 'empty')


if __name__ == '__main__':
    unittest.main()
