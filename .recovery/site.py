from pathlib import Path
import re
import subprocess
import sys

SOURCE = 'a2bbb71c05be39272b7efff77197bd8a080f2574'
def write(path, text):
    p = Path(path); p.parent.mkdir(parents=True, exist_ok=True); p.write_text(text)
def old(path):
    return subprocess.check_output(['git', 'show', SOURCE + ':' + path], text=True)

TESTS = r'''"""Regression coverage for static, recorded MAG walkthroughs."""
import functools
import http.server
import os
from pathlib import Path
import threading
import unittest

ROOT = Path(__file__).resolve().parents[1]
DEMOS = {'retrieval-pipeline': 12, 'cross-tool-handoff': 7, 'abstention': 6}

class RecoveryFilesTests(unittest.TestCase):
    def test_recovered_pages_are_present_and_disclosed(self):
        self.assertTrue((ROOT / 'site/demos/index.html').is_file(), 'demo index is missing')
        for slug in DEMOS:
            text = (ROOT / f'site/demos/{slug}.html').read_text()
            self.assertIn('Recorded walkthrough', text)
            self.assertIn('not a live MAG session', text)
            self.assertIn('src="../assets/demo-player.js"', text)
        self.assertTrue((ROOT / 'site/assets/pipeline.svg').is_file())

class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass

@unittest.skipUnless(os.environ.get('MAG_TEST_BROWSER') == '1', 'opt-in browser dependencies')
class DemoBrowserTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        from playwright.sync_api import sync_playwright
        handler = functools.partial(QuietHandler, directory=str(ROOT / 'site'))
        cls.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base = f'http://127.0.0.1:{cls.server.server_port}'
        cls.pw = sync_playwright().start()
        cls.browser = cls.pw.chromium.launch(headless=True)

    @classmethod
    def tearDownClass(cls):
        cls.browser.close(); cls.pw.stop()
        cls.server.shutdown(); cls.server.server_close(); cls.thread.join()

    def open_page(self, slug, **options):
        page = self.browser.new_page(**options)
        self.addCleanup(page.close)
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        response = page.goto(f'{self.base}/demos/{slug}.html')
        self.assertEqual(response.status, 200)
        return page, errors

    def test_playback_scrubbing_and_deep_links(self):
        for slug, total in DEMOS.items():
            with self.subTest(demo=slug):
                page, errors = self.open_page(slug)
                self.assertEqual(page.locator('#steps button').count(), total)
                self.assertTrue(page.locator('#first').is_disabled())
                self.assertTrue(page.locator('#prev').is_disabled())
                page.locator('#next').click()
                self.assertEqual(page.locator('#scrubber').input_value(), '2')
                self.assertEqual(page.locator('[aria-current="step"]').count(), 1)
                page.locator('#last').click()
                self.assertEqual(page.locator('#scrubber').input_value(), str(total))
                self.assertTrue(page.locator('#next').is_disabled())
                final_url = page.url
                page.reload()
                self.assertEqual(page.locator('#scrubber').input_value(), str(total))
                self.assertEqual(page.url, final_url)
                page.locator('#first').click()
                page.locator('#scrubber').focus(); page.keyboard.press('ArrowRight')
                self.assertEqual(page.locator('#scrubber').input_value(), '2')
                page.clock.install()
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
                    if os.environ.get('MAG_SCREENSHOTS'):
                        out = Path(os.environ['MAG_SCREENSHOTS']); out.mkdir(parents=True, exist_ok=True)
                        page.locator('#first').click()
                        page.screenshot(path=str(out / f'{slug}-{width}.png'), full_page=True)

if __name__ == '__main__':
    unittest.main()
'''
write('tests/test_site_recovery.py', TESTS)
if sys.argv[1] == 'tests':
    sys.exit(0)

for name in ('index', 'retrieval-pipeline', 'cross-tool-handoff', 'abstention'):
    path = 'site/demos/' + name + '.html'
    text = old(path)
    text = text.replace('Local-first memory for MCP clients.', 'Local-first memory. CLI first; MCP optional.')
    note = '<p class="note">Recorded walkthrough, not a live MAG session. The displayed capture dates from 4 September 2026 and may differ from current builds. <a href="capture-notes.html">Source and verification notes</a>.</p>'
    text = text.replace('<p class="eyebrow">Demo', note + '\n<p class="eyebrow">Demo', 1)
    if name == 'index':
        text = text.replace('Every number on these pages was measured by running the MAG binary against a three-memory database, not written to look good.', 'They replay observations reported by the original author from a three-memory CLI session; they do not run MAG in your browser.')
    else:
        start = text.index('var cur =')
        body = text[start:]
        prefix = body[:body.index('STATES.forEach(')]
        prefix = re.sub(r'var cur = 0, playing = false, timer = null, lastBar = \{\};', 'var lastBar = {};', prefix)
        prefix = prefix.replace('var cur = 0, playing = false, timer = null;\n', '')
        prefix = prefix[:prefix.index('function reduced()')] + 'var reduced = MagDemoPlayer.reduced;\nvar pad = MagDemoPlayer.pad;\nvar esc = MagDemoPlayer.escape;\n\n'
        render = body[body.index('function buildRow('):body.index('function go(')]
        render = render.replace('function render(){', 'function render(cur){')
        render = render[:render.index('  Array.prototype.forEach.call(stepBtns,')] + '}\n\n'
        text = text[:start] + prefix + render + 'MagDemoPlayer.mount(STATES, render);\n})();\n</script>\n</body>\n</html>\n'
        text = text.replace('<script>', '<script src="../assets/demo-player.js"></script>\n<script>', 1)
    # Long command output remains horizontally scrollable inside the component.
    text = text.replace('</style>', '\n.stage,.cand,.rowc,.stage-head>div{min-width:0}.term,.sig,.facts{overflow-wrap:anywhere}.rowc>div,.cand>div{min-width:0}\n</style>')
    write(path, text)
write('site/assets/pipeline.svg', old('site/assets/pipeline.svg'))
write('site/assets/demo-player.js', r'''/* Shared playback for recorded walkthroughs. No MAG runtime or network calls. */
(function (global) {
  "use strict";
  function reduced() {
    return !!(global.matchMedia && global.matchMedia("(prefers-reduced-motion: reduce)").matches);
  }
  function pad(n) { return (n < 10 ? "0" : "") + n; }
  function escape(text) {
    return String(text).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }
  function mount(states, render) {
    var current = 0, timer = null, el = {};
    ["steps", "scrubber", "scrubval", "play", "prev", "next", "first", "last"].forEach(function (key) {
      el[key] = document.getElementById(key);
    });
    var key = "mag-demo-play:" + global.location.pathname.split("/").pop();
    function save(value) {
      try { global.localStorage.setItem(key, value ? "1" : "0"); } catch (_) { /* storage is optional */ }
    }
    function saved() {
      try { return global.localStorage.getItem(key) === "1"; } catch (_) { return false; }
    }
    function stop() {
      if (timer !== null) { global.clearInterval(timer); timer = null; }
      el.play.setAttribute("aria-pressed", "false");
      el.play.textContent = "\u25b6 Play";
      save(false);
    }
    function go(index, keepHash) {
      current = Math.max(0, Math.min(states.length - 1, index));
      render(current);
      Array.prototype.forEach.call(el.steps.children, function (button, i) {
        if (i === current) { button.setAttribute("aria-current", "step"); }
        else { button.removeAttribute("aria-current"); }
      });
      el.scrubber.value = String(current + 1);
      el.scrubval.textContent = pad(current + 1) + " / " + pad(states.length);
      el.prev.disabled = el.first.disabled = current === 0;
      el.next.disabled = el.last.disabled = current === states.length - 1;
      if (!keepHash) {
        try { global.history.replaceState(null, "", "#" + states[current].id); }
        catch (_) { global.location.hash = states[current].id; }
      }
      if (current === states.length - 1 && timer !== null) { stop(); }
    }
    function start() {
      if (timer !== null || current === states.length - 1) { return; }
      el.play.setAttribute("aria-pressed", "true");
      el.play.textContent = "\u275a\u275a Pause";
      save(true);
      timer = global.setInterval(function () { go(current + 1); }, 3600);
    }
    function choose(index) { stop(); go(index); }
    function hashIndex() {
      return states.findIndex(function (state) { return state.id === global.location.hash.slice(1); });
    }
    states.forEach(function (state, index) {
      var button = document.createElement("button");
      button.type = "button"; button.className = "step";
      button.textContent = pad(index + 1) + " " + state.name;
      button.addEventListener("click", function () { choose(index); });
      el.steps.appendChild(button);
    });
    el.scrubber.max = String(states.length);
    el.play.addEventListener("click", function () { if (timer !== null) { stop(); } else { start(); } });
    el.next.addEventListener("click", function () { choose(current + 1); });
    el.prev.addEventListener("click", function () { choose(current - 1); });
    el.first.addEventListener("click", function () { choose(0); });
    el.last.addEventListener("click", function () { choose(states.length - 1); });
    el.scrubber.addEventListener("input", function () {
      var value = Number(el.scrubber.value);
      if (Number.isInteger(value)) { choose(value - 1); }
    });
    global.addEventListener("hashchange", function () {
      var index = hashIndex();
      if (index >= 0 && index !== current) { stop(); go(index, true); }
    });
    global.addEventListener("pagehide", function () {
      if (timer !== null) { global.clearInterval(timer); timer = null; }
    });
    var index = hashIndex();
    go(index >= 0 ? index : 0, index >= 0);
    if (!reduced() && saved()) { start(); }
  }
  global.MagDemoPlayer = { mount: mount, reduced: reduced, pad: pad, escape: escape };
})(window);
''')
text = Path('site/demos/index.html').read_text()
a = text.index('<main id="main">'); b = text.index('</main>', a) + len('</main>')
text = text[:a] + '''<main id="main"><section><div class="wrap">
<p class="eyebrow">Capture notes</p><h1>Recorded data,<br>not a live session.</h1>
<p class="lead">The walkthroughs preserve the original author's CLI examples from 4 September 2026. Playback runs in your browser; MAG does not.</p>
<h2 style="margin-top:44px">Source</h2>
<p>Recovered from <code>claude/mag-workflows-demos-dprysm</code> at <a href="https://github.com/George-RD/mag/commit/a2bbb71c05be39272b7efff77197bd8a080f2574"><code>a2bbb71</code></a>. The original standalone capture archive and producing binary hash were not checked in. Embedded numbers are historical observations reported by that author, not independently authenticated measurements.</p>
<p>CLI commands represent two tools sharing a database. Claude Code and Cursor are illustrative roles, not evidence of an automated session inside those clients. UUIDs, timestamps and scores depend on the run and corpus.</p>
<h2 style="margin-top:44px">Current boundary</h2>
<p>This recovery does not promote a model, change retrieval thresholds or qualify generative ingestion. The existing extraction evaluation remains separate. Current verification and remaining recovery work are recorded in <a href="https://github.com/George-RD/mag/blob/main/meta/reviews/claude-branch-recovery.md">the recovery review</a>.</p>
<p><a href="index.html">Return to the walkthroughs</a></p>
</div></section></main>''' + text[b:]
text = text.replace('<title>MAG demos — retrieval, handoff, abstention</title>', '<title>MAG demo capture notes</title>')
write('site/demos/capture-notes.html', text)
p = Path('site/index.html'); text = p.read_text().replace('<a href="#mechanism">How it works</a>', '<a href="#mechanism">How it works</a><a href="demos/index.html">Demos</a>'); write(p, text)
p = Path('README.md'); write(p, p.read_text() + '\nRecorded CLI walkthroughs: [retrieval, handoff and abstention](https://george-rd.github.io/mag/demos/). These are recorded examples, not live browser execution.\n')
text = old('docs/writing-style.md').replace('then abstains when the top score falls below the cutoff.', 'then applies a lexical-overlap gate with a narrow semantic rescue.')
write('docs/writing-style.md', text)
text = old('docs/architecture.md')
text = text.replace('<!-- Last verified: 2026-09-04 | Valid for: v0.1.10-dev+ -->', '<!-- Retrieval corrections checked against 09c8634, 2026-09-10. Historical performance is dated separately. -->')
text = text.replace('No external services, no network calls at query time.', 'Core retrieval runs locally after model installation. First use may download artifacts; opt-in generation uses a separate HTTP backend.')
text = text.replace('Embedding generation (ONNX, 7 ms for the default model)', 'Embedding generation (local ONNX)').replace('query embedding (7 ms)', 'query embedding')
text = text.replace('Exact semantic duplicates are caught here with zero compute cost.', 'Identical canonical text is caught before embedding inference.')
text = text.replace('character 3-gram Jaccard similarity', 'Jaccard similarity over stemmed, stopword-filtered token sets (minimum word length 3)').replace('3-gram Jaccard between query and candidate', 'token-set Jaccard, minimum word length 3')
text = re.sub(r'Scored against hand-authored ground truth, this extractor.*?\n\n', 'These rules can mistake tools for people. The recorded demo shows a historical example, not a current aggregate accuracy claim.\n\n', text)
text = text[:text.index('## Measured Results')] + '''## Evidence boundaries

[Recorded CLI walkthroughs](https://george-rd.github.io/mag/demos/) explain
retrieval and retain known failure examples. Their capture is historical; see
the accompanying capture notes. They are not a live service or a model-quality
benchmark.

The [current extraction evaluation](../benches/memory_intelligence/README.md)
measures the opt-in CLI producer separately. It does not enable or qualify
generative ingestion. Retrieval quality is measured by LoCoMo and LongMemEval;
see the [benchmark methodology](benchmarks/methodology.md).

Corrections were checked against `scoring.rs`, SQLite `crud.rs`, `entities.rs`,
`search.rs`, `conn_pool.rs`, `pipeline/` and release feature configuration at
`09c8634`. Current embedding profiles and migration safeguards remain in force;
see [re-embedding](re-embedding.md).
'''
write('docs/architecture.md', text)
# Preserve source audit as history, never as fresh exemptions or task status.
audit = old('meta/todos/review-and-split-oversized-modules.md').split('---', 2)[2].lstrip()
write('docs/recovery/claude-module-audit-2026-09-04.md', '# Historical module audit\n\nRecovered from `a2bbb71` on 10 September 2026. The text below describes the\nsource branch on 4 September; line counts, status and proposed splits are not\nauthority for current main. No allow markers or generated Cairn state are imported.\n\n---\n\n' + audit)
p = Path('meta/todos/review-and-split-oversized-modules.md'); write(p, p.read_text() + '\n## Recovered audit input\n\nThe 4 September source-branch classification is preserved in\n`docs/recovery/claude-module-audit-2026-09-04.md`. Revalidate it against the current\ncode before changing this todo or adding exemptions. Recovery itself does not\nperform the proposed module splits.\n')
write('meta/reviews/claude-branch-recovery.md', '''---
node: mag.quality.scripts
review_type: agent_introspective
date: 2026-09-10
reviewer: OpenAI coding assistant
---
# Claude branch recovery

Source: `claude/mag-workflows-demos-dprysm` at
`a2bbb71c05be39272b7efff77197bd8a080f2574`.
Base: `09c863459cfb5943a58ecde306d303244df69b12`.
George approved focused recovery on 10 September 2026. PR #449 is independent
and remains untouched.

## Recovered boundary

Three recorded demos, the pipeline diagram, selected architecture corrections,
the writing guide and the historical module-cohesion audit are retained. A shared
playback controller replaces three copies of timer, hash and control logic.
Tests cover the 12/7/6 states, navigation, scrubbing, deep links, live-region
updates, reduced-motion startup and desktop/mobile overflow.

The original author did not commit a standalone demo capture or binary hash.
Every demo now discloses that limitation. Historical numbers are not promoted to
current evidence. CLI commands illustrate client roles; actual client sessions
were not measured here. Existing homepage roadmap statuses are not overwritten.
The architecture port also corrects inherited character-3-gram claims: the
production Jaccard implementation compares token sets with minimum word length 3.

## Verification

The missing demo index is a test-first failure before recovery. Browser and
static-site checks run against the recovered tree. Exact-head CI/Cairn and review
results must be recorded in the PR before merge. A hosted build of unchanged
main is available from workbench run 34453281349 for fresh CLI observations;
that build alone does not verify the recovered JavaScript.

Ownership: tests under `mag.quality.tests`, validation under
`mag.quality.scripts`. No runtime, model, MCP, scorer or dataset is changed.
Generated maps/cache stamps and blanket large-module markers are not recovered.

## Remaining recovery

Adapt the 36-seed runtime-behaviour harness separately under the existing
benchmark todo. Reuse the shared production BGE adapter, use distinct paths from
the current extraction harness, avoid case-colliding documentation names and
rerun against current code. Keep its historical failures as hypotheses until
reproduced. The source branch remains intact until all recovery is accounted for.
''')
# Reuse existing Pages metadata/SVG/PNG validator as a normal script.
workflow = Path('.github/workflows/pages.yml').read_text()
a = workflow.index("          python3 - <<'PY'\n") + len("          python3 - <<'PY'\n")
b = workflow.index('\n          PY', a)
validator = '\n'.join(line[10:] for line in workflow[a:b].splitlines()) + '\n'
validator = validator.replace('html = site / "index.html"', 'html = site / "index.html"')
extra = r'''
# Validate every recovered page and all relative links without network requests.
from urllib.parse import urlsplit, unquote
root = site.resolve()
names = {}
for path in site.rglob('*'):
    key = str(path.relative_to(site)).casefold()
    if key in names:
        raise SystemExit(f'Case-insensitive path collision: {path} and {names[key]}')
    names[key] = path
class LinkParser(HTMLParser):
    def handle_starttag(self, tag, attrs):
        for key, value in attrs:
            if key in ('href', 'src') and value:
                link = urlsplit(value)
                if not link.scheme and not link.netloc and link.path:
                    target = (self.base / unquote(link.path)).resolve()
                    if not target.is_relative_to(root) or not target.exists():
                        raise SystemExit(f'Broken local link in {self.base}: {value}')
for page in sorted(site.rglob('*.html')):
    parser = Parser(); parser.feed(page.read_text())
    if not all((parser.lang, parser.title, parser.description)):
        raise SystemExit(f'{page}: missing page metadata')
    links = LinkParser(); links.base = page.parent; links.feed(page.read_text())
print('All demo metadata and local links passed')
'''
write('scripts/validate_site.py', validator + extra)
start = workflow.index('      - name: Validate static site\n')
end = workflow.index('\n  publish:', start)
workflow = workflow[:start] + '''      - name: Validate static site and recovered files
        run: |
          python3 scripts/validate_site.py
          python3 -m unittest tests.test_site_recovery -v
          node --check site/assets/demo-player.js
      - name: Install browser test dependencies
        run: |
          python3 -m pip install playwright==1.57.0
          python3 -m playwright install --with-deps chromium
      - name: Test recorded walkthroughs in Chromium
        env:
          MAG_TEST_BROWSER: '1'
        run: python3 -m unittest tests.test_site_recovery -v
''' + workflow[end:]
workflow = workflow.replace('permissions:\n  contents: write', 'permissions:\n  contents: read')
workflow = workflow.replace('  publish:\n', '  publish:\n    permissions:\n      contents: write\n')
workflow = workflow.replace('      - "site/**"', '      - "site/**"\n      - "scripts/validate_site.py"\n      - "tests/test_site_recovery.py"')
write('.github/workflows/pages.yml', workflow)
