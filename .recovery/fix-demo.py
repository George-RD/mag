from pathlib import Path
p=Path('site/assets/demo-player.js')
s=p.read_text().replace('var current = 0, timer = null;','var current = 0, timer = null, resumeOnShow = false;')
s=s.replace('function stop() {\n      if (timer', 'function stopTimer() {\n      if (timer').replace('el.play.textContent = "\\u25b6 Play";\n      save(false);','el.play.textContent = "\\u25b6 Play";\n    }\n    function stop() {\n      stopTimer();\n      save(false);')
s=s.replace('global.addEventListener("pagehide", function () {\n      if (timer !== null) { global.clearInterval(timer); timer = null; }\n    });','''global.addEventListener("pagehide", function () {
      resumeOnShow = timer !== null;
      stopTimer(); // Preserve the explicit preference across reloads.
    });
    global.addEventListener("pageshow", function (event) {
      if (event.persisted) {
        if (resumeOnShow && !reduced()) { start(); }
        resumeOnShow = false;
      }
    });''')
p.write_text(s)
p=Path('site/demos/cross-tool-handoff.html'); s=p.read_text(); assert s.count('3-gram Jaccard')==1
p.write_text(s.replace('3-gram Jaccard','stemmed, stopword-filtered token Jaccard'))
p=Path('tests/test_site_demos.py');s=p.read_text()
p.write_text(s.replace("if __name__ == '__main__':", "def load_tests(loader, tests, pattern):\n    from tests import test_demo_player_lifecycle\n    tests.addTests(loader.loadTestsFromModule(test_demo_player_lifecycle))\n    return tests\n\n\nif __name__ == '__main__':"))
p=Path('meta/reviews/claude-branch-recovery.md')
p.write_text(p.read_text()+'''
## Review follow-up

Codex review 5164832533 on `e03e6db` found a suspended playback timer after
back-forward-cache restoration (3977195151) and a stale character-3-gram
explanation in the handoff demo (3977195164). Four new tests were run before
the fixes: active restoration, reduced-motion state, and algorithm wording
failed; the paused-state control passed. After the fix, all four pass locally
with Chromium. Lifecycle tests dispatch persisted page transition events so
they do not depend on Chromium choosing to admit a particular page to bfcache.
The five separate served-page browser tests remain in Pages CI.

Suspension now clears the timer and visible Pause state while remembering
whether playback was active. A persisted restore resumes once unless reduced
motion is requested; an explicit pause stays paused. The handoff explanation
now matches the stemmed, stopword-filtered token-set comparison in production.
No historical capture values or production algorithms changed.

Initial exact-head CI 34455870583, Pages 34455870618, evaluation 34455870625 and
Cairn 34455870562 passed. The amended head requires its own checks and review.
''')
