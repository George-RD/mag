from pathlib import Path
import subprocess
p=Path('benches/runtime_behaviour/families/supersession.rs')
s=p.read_text()
s+=r'''
#[cfg(test)]
mod tests {
    use super::*;
    use mag::LocalMemoryRuntime;
    use mag::memory_core::MemoryInput;
    use mag::memory_core::embedder::PlaceholderEmbedder;
    use std::sync::Arc;

    async fn assert_edge_direction(reversed: bool, expected: bool) {
        let directory = tempfile::TempDir::new().unwrap();
        let runtime = LocalMemoryRuntime::new_with_path(
            directory.path().join("edge.db"), Arc::new(PlaceholderEmbedder),
        ).unwrap();
        let older = uuid::Uuid::new_v4().to_string();
        let newer = uuid::Uuid::new_v4().to_string();
        runtime.store_raw(&older, "Amber telescope calibration", &MemoryInput::new()).await.unwrap();
        runtime.store_raw(&newer, "Cobalt orchard irrigation", &MemoryInput::new()).await.unwrap();
        let (source, target) = if reversed { (&newer, &older) } else { (&older, &newer) };
        runtime.add_relationship(source, target, "SUPERSEDES", 1.0, &json!({})).await.unwrap();
        let group = SeededGroup {
            runtime,
            key_to_id: BTreeMap::from([("old".into(), older.clone()), ("new".into(), newer.clone())]),
            id_to_key: BTreeMap::from([(older.clone(), "old".into()), (newer.clone(), "new".into())]),
            content_to_key: BTreeMap::new(),
            seeded: 2,
            retained: 2,
            retained_ids: [older, newer].into_iter().collect(),
        };
        let case = SupersessionCase {
            old: "old".into(), new: "new".into(), expect_supersession: expected,
            kind: "edge_only".into(), note: None,
        };
        let result = supersession(&[SupersessionPair { case: &case, group: &group }]).await.unwrap();
        assert_eq!(result.detail["cases"][0]["detected_via_version_chain"], false);
        assert_eq!(result.detail["cases"][0]["detected_via_supersedes_edge"], expected);
        assert_eq!(result.detail["cases"][0]["detected"], expected);
    }

    #[tokio::test]
    async fn retired_to_current_edge_is_recognized_without_a_version_chain() {
        assert_edge_direction(false, true).await;
    }

    #[tokio::test]
    async fn current_to_retired_edge_does_not_count_as_supersession() {
        assert_edge_direction(true, false).await;
    }
}
'''
p.write_text(s)
red=subprocess.run(['cargo','test','--locked','--no-default-features','--bin','memory_runtime_eval','families::supersession::tests'],capture_output=True,text=True)
print((red.stdout+red.stderr)[-7000:])
assert red.returncode != 0 and '2 failed' in red.stdout, 'Expected both directional assertions to fail before correction'
old='''                    && edge.source_id == *new_id
                    && edge.target_id == *old_id'''
new='''                    && edge.source_id == *old_id
                    && edge.target_id == *new_id'''
assert old in s
p.write_text(s.replace(old,new))
p=Path('benches/runtime_behaviour/baselines/2026-09-10-bge-small/README.md')
p.write_text(p.read_text()+'''\n## Inherited observation defect

The preserved producing revision checked `SUPERSEDES` edges backwards. Thus its
`detected_via_supersedes_edge` booleans are not reliable: the executable production
contract links the retired row to its replacement. The lexical case still scored
through its independently observed version chain. The recovered evaluator now has
two runtime-backed direction regressions and checks the correct orientation.
Original JSON bytes remain unchanged; this is a disclosed measurement correction,
not a production change or a favorable replacement run.
''')
p=Path('meta/reviews/runtime-behaviour-recovery.md')
p.write_text(p.read_text()+'''\n## Supersession observer correction

An introspective source check found the inherited family reversing `SUPERSEDES`:
production `crud.rs` calls `add_relationship(old_id, new_id, ...)`, whereas the
old observer required new-to-old. Two tests store actual rows through
`LocalMemoryRuntime`, add each directed edge without a version-chain mutation,
and fail before the comparison is corrected. This isolates the edge signal
rather than letting the existing OR with version-chain evidence hide the error.
The original archived run remains byte-identical, with its invalid edge-detail
booleans explicitly qualified beside it. No production or dataset change.
''')
