# Design: Purge jj and Restore Git Workflows

## Context

The project previously used jj (Jujutsu) in colocated mode. The team has moved back to standard git. All active documentation, scripts, and skills still reference jj commands, which is confusing for contributors and incorrect for current workflow.

## Goal

Remove all jj references from active workflow files and restore standard git workflows. Keep changes mechanical and focused — do not redesign unrelated systems.

## Scope

### In scope
- Update active workflow documentation to describe standard git commands
- Update `scripts/bump-version.sh` to use git commit + tag instead of jj describe + jj new
- Update `.claude/skills/release/SKILL.md` to use git instead of jj
- Update `docs/RELEASING.md` to use git tag + push
- Update `docs/specs/execution-roadmap.md` to reference git branches instead of jj bookmarks
- Light edits to `docs/strongholds/*.md` where jj references affect current workflow understanding
- Remove or archive `.claude/skills/jj-vcs-comprehensive/`

### Out of scope
- Test speed improvements
- CI pipeline redesign
- prek.toml changes (prek is a git hook framework; it works with git)
- Full rewrite of historical roadmap docs (light reference updates only)
- Changing actual source code logic

## Target Git Workflow

### Daily workflow
1. Create branch: `git switch -c feat/name` (or `git checkout -b feat/name`)
2. Make changes
3. Commit: `git commit -m "type(scope): description"`
4. Push: `git push -u origin feat/name`
5. Open PR: `gh pr create --head feat/name --title "..." --body "..."`
6. Merge via GitHub (merge or squash)

### Version bump workflow
1. Update versions in manifests (Cargo.toml, npm/package.json, python/pyproject.toml)
2. Commit: `git commit -am "chore: bump version to vX.Y.Z"`
3. Tag: `git tag vX.Y.Z`
4. Push tag: `git push origin vX.Y.Z`

### Cleanup after merge
- Delete local branch: `git branch -d feat/name`
- Delete remote branch: `git push origin --delete feat/name` (optional, GitHub can auto-delete)

## Files and Changes

| File | Change |
|------|--------|
| `AGENTS.md` | Rewrite Version Control section for git. Update PR workflow. Remove jj skill reference. Update commands section. |
| `CONTRIBUTING.md` | Replace jj setup with git. Update PR workflow steps. |
| `scripts/bump-version.sh` | Replace `jj describe` + `jj new` with `git commit` + `git tag`. Keep existing manifest update logic. |
| `.claude/skills/release/SKILL.md` | Replace `/jj` skill references with standard git commands. |
| `docs/RELEASING.md` | Update release steps to use git tag + push instead of jj. |
| `docs/specs/execution-roadmap.md` | Replace "jj bookmarks" / "jj rebase" with git branches. |
| `docs/strongholds/*.md` | Update jj references where they describe current workflow expectations. |
| `plugin/scripts/pre-compact.sh` | Change VCS detection priority from jj to git. |
| `plugin/scripts/commit-capture.sh` | Simplify: remove jj-specific fallback parsing (keep git capture). |
| `plugin/dev/scripts/pre-compact.sh` | Same as plugin/scripts/pre-compact.sh. |
| `plugin/dev/scripts/commit-capture.sh` | Same as plugin/scripts/commit-capture.sh. |
| `conductor/campaign-audit-remediation.md` | Light edit: update active workflow references. |
| `.claude/skills/jj-vcs-comprehensive/` | Remove skill directory entirely. |

## Approach

Restructure + Purge (Approach B): rewrite active workflow docs cleanly, keep mechanical edits pragmatic on historical files.

## Risk and Mitigation

| Risk | Mitigation |
|------|------------|
| Missed jj references | Grep for `jj `, `jujutsu`, `jj-`, `.jj` after changes |
| bump-version.sh breaks | Test the script in dry-run mode after editing |
| Release skill becomes inconsistent | Read full skill file, update all jj references holistically |

## Success Criteria

- `grep -ri "jj " AGENTS.md CONTRIBUTING.md scripts/bump-version.sh docs/RELEASING.md plugin/scripts/` returns no workflow-relevant hits
- `grep -ri "jujutsu" AGENTS.md CONTRIBUTING.md` returns nothing
- `.claude/skills/jj-vcs-comprehensive/` no longer exists
- `scripts/bump-version.sh` completes a dry-run successfully
- All quality gates still pass (`cargo fmt`, `cargo clippy`, `cargo test`)
