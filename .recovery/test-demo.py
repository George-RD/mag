"""Deterministic playback lifecycle tests; bfcache admission is browser-owned."""
import os
from pathlib import Path
import unittest

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]


class DemoPlayerLifecycleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.playwright = sync_playwright().start()
        options = {'headless': True}
        if os.environ.get('MAG_BROWSER_PATH'):
            options['executable_path'] = os.environ['MAG_BROWSER_PATH']
        cls.browser = cls.playwright.chromium.launch(**options)

    @classmethod
    def tearDownClass(cls):
        cls.browser.close()
        cls.playwright.stop()

    def page(self):
        page = self.browser.new_page()
        self.addCleanup(page.close)
        page.clock.install()
        markup = (ROOT / 'site/demos/cross-tool-handoff.html').read_text()
        script = (ROOT / 'site/assets/demo-player.js').read_text()
        page.set_content(markup.replace('<script src="../assets/demo-player.js"></script>',
                                        '<script>' + script + '</script>'))
        return page

    def transition(self, page, name):
        page.evaluate('(name) => dispatchEvent(new PageTransitionEvent(name, {persisted: true}))', name)

    def test_active_playback_resumes_once_after_restore(self):
        page = self.page()
        page.locator('#play').click()
        self.transition(page, 'pagehide')
        page.clock.fast_forward(4000)
        self.assertEqual(page.locator('#scrubber').input_value(), '1')
        self.transition(page, 'pageshow')
        self.transition(page, 'pageshow')
        page.clock.fast_forward(3700)
        self.assertEqual(page.locator('#scrubber').input_value(), '2')
        self.assertEqual(page.locator('#play').get_attribute('aria-pressed'), 'true')
        page.locator('#play').click()
        page.clock.fast_forward(4000)
        self.assertEqual(page.locator('#scrubber').input_value(), '2')
        self.assertEqual(page.locator('#play').get_attribute('aria-pressed'), 'false')

    def test_paused_playback_stays_paused_after_restore(self):
        page = self.page()
        self.transition(page, 'pagehide')
        self.transition(page, 'pageshow')
        page.clock.fast_forward(4000)
        self.assertEqual(page.locator('#scrubber').input_value(), '1')
        self.assertEqual(page.locator('#play').get_attribute('aria-pressed'), 'false')

    def test_reduced_motion_restore_clears_stale_pause_control(self):
        page = self.page()
        page.locator('#play').click()
        self.transition(page, 'pagehide')
        page.emulate_media(reduced_motion='reduce')
        self.transition(page, 'pageshow')
        page.clock.fast_forward(4000)
        self.assertEqual(page.locator('#scrubber').input_value(), '1')
        self.assertEqual(page.locator('#play').get_attribute('aria-pressed'), 'false')
        self.assertIn('Play', page.locator('#play').text_content())


class DemoAlgorithmCopyTests(unittest.TestCase):
    def test_handoff_describes_token_not_character_jaccard(self):
        markup = (ROOT / 'site/demos/cross-tool-handoff.html').read_text()
        self.assertNotIn('3-gram Jaccard', markup)
        self.assertIn('stemmed, stopword-filtered token Jaccard', markup)


if __name__ == '__main__':
    unittest.main()
