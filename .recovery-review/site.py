from pathlib import Path
import subprocess

assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip() == 'e63da1fb6d5d3ab57454843944aa71e433a53794'
p=Path('tests/test_demo_player_lifecycle.py')
s=p.read_text()
insert='''
    def test_hash_labels_distinguish_stored_raw_hash_from_dedup(self):
        markup = (ROOT / 'site/demos/cross-tool-handoff.html').read_text()
        self.assertIn('raw-content checksum; not queried by deduplication', markup)
        self.assertIn('exact equality after canonicalisation', markup)
        self.assertNotIn('near-duplicate detection after normalising', markup)

    def test_schema_walkthrough_is_explicitly_partial(self):
        markup = (ROOT / 'site/demos/cross-tool-handoff.html').read_text()
        self.assertIn('Selected column groups', markup)
        self.assertNotIn('This is the whole record', markup)
        self.assertNotIn('embedding is a tenth column', markup)

    def test_pages_triggers_for_lifecycle_regressions(self):
        workflow = (ROOT / '.github/workflows/pages.yml').read_text()
        for event in ('push', 'pull_request'):
            section = workflow.split('  ' + event + ':', 1)[1].split('  workflow_dispatch:', 1)[0]
            if event == 'push':
                section = section.split('  pull_request:', 1)[0]
            self.assertIn('"tests/test_demo_player_lifecycle.py"', section)
'''
assert "\n\nif __name__ == '__main__':" in s
p.write_text(s.replace("\n\nif __name__ == '__main__':",'\n'+insert+"\n\nif __name__ == '__main__':"))
result=subprocess.run(['python3','-m','unittest','tests.test_demo_player_lifecycle.DemoAlgorithmCopyTests','-v'],capture_output=True,text=True)
print(result.stdout+result.stderr)
assert result.returncode != 0 and 'failures=3' in result.stderr
p=Path('site/demos/cross-tool-handoff.html'); s=p.read_text().replace('This is the whole record; there is no second store holding the interesting half.', 'These are selected column groups, not the complete schema; tags are shown in the next state.').replace('setLabel:"Columns of the stored row"','setLabel:"Selected column groups"').replace('"exact-duplicate detection"','"raw-content checksum; not queried by deduplication"').replace('"near-duplicate detection after normalising"','"exact equality after canonicalisation"').replace("Nine of the row's columns carry the record. The embedding is a tenth column on the same row: a BLOB of 384 floats, not a separate table.", 'The groups above show selected fields. The embedding is another column on the same row: a BLOB of 384 floats, not a separate table. Other schema fields are omitted here.'); p.write_text(s)
p=Path('site/demos/retrieval-pipeline.html'); p.write_text(p.read_text().replace('values is','values are'))
p=Path('.github/workflows/pages.yml'); p.write_text(p.read_text().replace('      - "tests/test_site_demos.py"', '      - "tests/test_site_demos.py"\n      - "tests/test_demo_player_lifecycle.py"'))
p=Path('docs/architecture.md'); s=p.read_text().replace('dual_match_boost + 0.5 / (1 + fts_rank) -- 2.0 at FTS rank 0,','max(dual_match_boost, 1.0) + 0.5 / (1 + fts_rank) -- 2.0 at FTS rank 0,'); s += '\n`pipeline/fusion.rs` computes the dual-match multiplier from the configured\nbase (clamped to at least 1.0) plus the inverse-rank term. Its inherited 1.3–1.8\ncomment is stale: the default base is 1.5, making the top-FTS multiplier 2.0.\nThe formula above describes executable behaviour, not that comment.\n'; p.write_text(s)
p=Path('meta/reviews/claude-branch-recovery.md'); p.write_text(p.read_text()+'''\n## Final review corrections

Codex comments 3977365164, 3977365170 and 3977365173 identified a missing
workflow path, incorrect raw/canonical hash labels and a partial-schema claim.
Three added regressions failed before the minimal fixes; the existing token
Jaccard assertion remained a passing control. Both Pages path filters now include
the lifecycle test module. The no-script grammar finding is corrected.

CodeRabbit comment 3977299422 conflicts with executable code:
`pipeline/fusion.rs:137-139` computes `max(dual_match_boost, 1.0) +
0.5 / (1 + fts_rank)`. The default base is 1.5, so rank 0 receives 2.0.
The old 1.3–1.8 comment is stale, not a separate adaptive/fallback path.
The documentation retains the actual formula and now includes its base clamp.
No scoring code is changed.
''')
