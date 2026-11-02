"""Static placement and narrow-column rules; not browser visual acceptance."""
from html.parser import HTMLParser
from pathlib import Path
import re
import unittest


class Layout(HTMLParser):
    def __init__(self, source):
        super().__init__()
        self.stack = []
        self.ids = {}
        self.tags = {}
        self.group_summaries = []
        self.feed(source.split('<script>', 1)[0])

    def handle_starttag(self, tag, attributes):
        attributes = dict(attributes)
        identity = attributes.get('id')
        if tag=='summary' and self.stack and self.stack[-1][1].get('class')=='inspector-section':
            self.group_summaries.append(self.stack[-1][1]['id'])
        if identity:
            if identity in self.ids:
                raise AssertionError(f'duplicate id {identity}')
            self.ids[identity] = (tuple(self.stack), attributes)
            self.tags[identity] = tag
        if tag not in {'meta', 'link', 'input', 'img', 'br', 'hr'}:
            self.stack.append((tag, attributes))

    def handle_endtag(self, tag):
        for index in range(len(self.stack) - 1, -1, -1):
            if self.stack[index][0] == tag:
                self.stack = self.stack[:index]
                break


class ViewerLayoutTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = Path(__file__).parents[1].joinpath('viewer.html').read_text(encoding='utf-8')
        cls.layout = Layout(cls.source)

    def test_memory_summary_is_in_inspector_outside_replaceable_detail(self):
        parents, attributes = self.layout.ids['gpu-memory-summary']
        self.assertTrue(any(tag == 'aside' and attrs.get('class') == 'inspector' for tag, attrs in parents))
        self.assertFalse(any(tag == 'main' or attrs.get('id') == 'detail' for tag, attrs in parents))
        self.assertNotIn('style', attributes)
        order = list(self.layout.ids)
        self.assertLess(order.index('frame-preview'), order.index('gpu-memory-summary'))
        self.assertLess(order.index('instruction'), order.index('gpu-memory-summary'))
        groups=['inspect-section','legend-section','bin-section','memory-section']
        self.assertEqual([identity for identity in order if identity in groups], groups)
        self.assertEqual(self.layout.group_summaries, groups)
        for identity in groups:
            self.assertEqual(self.layout.tags[identity], 'details')
            self.assertIn('open', self.layout.ids[identity][1])
        self.assertFalse(any(tag=='details' for tag,_ in self.layout.ids['frame-preview'][0]))
        self.assertTrue(any(attrs.get('id')=='memory-section' for _,attrs in parents))

    def test_hidden_frame_has_explicit_author_display_rule(self):
        # An author inline-block rule beats the browser's ordinary UA [hidden].
        # This ID+attribute selector has greater specificity than .frame-wrap.
        style = self.source.split('<style>', 1)[1].split('</style>', 1)[0]
        hidden = re.findall(r'#frame-wrap\[hidden\]\s*\{([^}]+)\}', style)
        self.assertEqual(len(hidden), 1)
        self.assertRegex(hidden[0], r'(^|;)\s*display\s*:\s*none\s*(;|$)')
        self.assertIn('.frame-wrap{display:inline-block}', style)
        self.assertNotIn('.frame-wrap[hidden]{display:block}', style)

    def test_memory_axis_and_scroll_remain_in_gpu_plot_pane(self):
        for identity in ['load-axis', 'load-scroll', 'load']:
            parents, _ = self.layout.ids[identity]
            self.assertTrue(any(tag == 'main' for tag, _ in parents))
            self.assertTrue(any(attrs.get('aria-label') == 'GPU Activity and Memory Throughput' for _, attrs in parents))
            self.assertFalse(any(tag == 'aside' for tag, _ in parents))

    def test_summary_grid_can_shrink_and_wrap_in_existing_responsive_sidebar(self):
        def rules(selector):
            return re.search(re.escape(selector) + r'\{([^}]+)\}', self.source)[1]
        section = rules('#gpu-memory-summary')
        self.assertIn('min-width:0', section)
        self.assertIn('overflow-wrap:anywhere', section)
        grid = rules('#gpu-memory-summary .gpu-memory-metrics')
        self.assertIn('display:grid', grid)
        self.assertIn('grid-template-columns:minmax(0,1fr)', grid)
        self.assertIn('min-width:0', rules('#gpu-memory-summary .gpu-memory-metrics>div'))
        self.assertIn('290px', self.source)
        self.assertIn('240px', self.source)
        self.assertIn('@media(max-width:700px)', self.source)
        self.assertIn('.layout{display:block;height:auto;min-height:0}', self.source)


if __name__ == '__main__':
    unittest.main()
