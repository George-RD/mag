"""Browser regressions for the recovered, recorded CLI walkthroughs."""
import functools
import http.server
import os
from pathlib import Path
import threading
import unittest

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]
DEMOS = {'retrieval-pipeline': 12, 'cross-tool-handoff': 7, 'abstention': 6}


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


class DemoBrowserTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        handler = functools.partial(QuietHandler, directory=str(ROOT / 'site'))
        cls.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base = f'http://127.0.0.1:{cls.server.server_port}'
        cls.playwright = sync_playwright().start()
        options = {'headless': True}
        if os.environ.get('MAG_BROWSER_PATH'):
            options['executable_path'] = os.environ['MAG_BROWSER_PATH']
        cls.browser = cls.playwright.chromium.launch(**options)

    @classmethod
    def tearDownClass(cls):
        cls.browser.close()
        cls.playwright.stop()
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()

    def open_page(self, slug, **options):
        page = self.browser.new_page(**options)
        self.addCleanup(page.close)
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.goto(f'{self.base}/demos/{slug}.html')
        return page, errors

    def test_playback_scrubbing_and_deep_links(self):
        for slug, total in DEMOS.items():
            with self.subTest(demo=slug):
                page, errors = self.open_page(slug)
                page.clock.install()
                self.assertEqual(page.locator('#steps button').count(), total)
                self.assertTrue(page.locator('#first').is_disabled())
                self.assertTrue(page.locator('#prev').is_disabled())
                page.locator('#next').click()
                self.assertEqual(page.locator('#scrubber').input_value(), '2')
                self.assertEqual(page.locator('[aria-current="step"]').count(), 1)
                page.locator('#last').click()
                self.assertEqual(page.locator('#scrubber').input_value(), str(total))
                self.assertTrue(page.locator('#next').is_disabled())
                self.assertTrue(page.locator('#last').is_disabled())
                final_url = page.url
                page.reload()
                self.assertEqual(page.locator('#scrubber').input_value(), str(total))
                self.assertEqual(page.url, final_url)
                page.locator('#first').click()
                page.locator('#scrubber').focus()
                page.keyboard.press('ArrowRight')
                self.assertEqual(page.locator('#scrubber').input_value(), '2')
                page.locator('#play').click()
                self.assertEqual(page.locator('#play').get_attribute('aria-pressed'), 'true')
                page.clock.fast_forward(3700)
                self.assertEqual(page.locator('#scrubber').input_value(), '3')
                page.locator('#prev').click()
                self.assertEqual(page.locator('#play').get_attribute('aria-pressed'), 'false')
                page.clock.fast_forward(4000)
                self.assertEqual(page.locator('#scrubber').input_value(), '2')
                self.assertTrue(page.locator('#live').inner_text().strip())
                self.assertEqual(errors, [])

    def test_explicit_playback_preference_survives_reload(self):
        page, errors = self.open_page('cross-tool-handoff')
        page.locator('#play').click()
        page.reload()
        self.assertEqual(page.locator('#play').get_attribute('aria-pressed'), 'true')
        page.locator('#play').click()
        page.reload()
        self.assertEqual(page.locator('#play').get_attribute('aria-pressed'), 'false')
        self.assertEqual(errors, [])

    def test_reduced_motion_does_not_restore_autoplay(self):
        for slug in DEMOS:
            with self.subTest(demo=slug):
                page, errors = self.open_page(slug, reduced_motion='reduce')
                page.evaluate("localStorage.setItem('mag-demo-play:' + location.pathname.split('/').pop(), '1')")
                page.reload()
                self.assertEqual(page.locator('#play').get_attribute('aria-pressed'), 'false')
                self.assertEqual(page.locator('#scrubber').input_value(), '1')
                self.assertEqual(errors, [])

    def test_all_states_fit_desktop_and_mobile(self):
        for width in (375, 1280):
            for slug, total in DEMOS.items():
                with self.subTest(width=width, demo=slug):
                    page, errors = self.open_page(slug, viewport={'width': width, 'height': 900}, reduced_motion='reduce')
                    for index in range(total):
                        page.locator('#scrubber').evaluate('(el, value) => { el.value = value; el.dispatchEvent(new Event("input")); }', str(index + 1))
                        self.assertLessEqual(page.evaluate('document.documentElement.scrollWidth'), width)
                        self.assertTrue(page.locator('#stagename').inner_text().strip())
                    self.assertEqual(errors, [])

    def test_index_links_and_historical_disclosure(self):
        page = self.browser.new_page()
        self.addCleanup(page.close)
        self.assertTrue((ROOT / 'site/demos/index.html').is_file(), 'demo index is missing')
        page.goto(f'{self.base}/demos/index.html')
        for slug in DEMOS:
            self.assertEqual(page.locator(f'a[href="{slug}.html"]').count(), 1)
        for slug in DEMOS:
            page.goto(f'{self.base}/demos/{slug}.html')
            self.assertIn('Recorded walkthrough', page.locator('main').inner_text())
            self.assertIn('not a live MAG session', page.locator('main').inner_text())
            self.assertEqual(page.locator('script[src="../assets/demo-player.js"]').count(), 1)


def load_tests(loader, tests, pattern):
    from tests import test_demo_player_lifecycle
    tests.addTests(loader.loadTestsFromModule(test_demo_player_lifecycle))
    return tests


if __name__ == '__main__':
    unittest.main()
