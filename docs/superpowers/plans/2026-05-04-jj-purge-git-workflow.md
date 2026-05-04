# Purge jj and Restore Git Workflows — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove all jj references from active workflow files and restore standard git workflows.

**Architecture:** Mechanical documentation and script updates. Replace jj commands with git equivalents across docs, scripts, and skills. No source code changes.

**Tech Stack:** Bash, Markdown, git

---

### Task 1: Rewrite AGENTS.md Version Control section

**Files:**
- Modify: `AGENTS.md:150-192`

- [ ] **Step 1: Replace jj VCS section with git workflow**

Replace the entire Version Control (jj) section and PR workflow with standard git instructions.

```markdown
## Version Control

This repo uses standard git.

- Use normal git commands: `git branch`, `git commit`, `git rebase`, `git checkout`, `git switch`
- Feature branches: `feat/...`, `fix/...`, `perf/...`, `refactor/...`

### PR workflow

1. `git switch -c feat/my-feature` (or `git checkout -b feat/my-feature`)
2. Make changes and commit: `git commit -m "feat(scope): description"`
3. Run quality gates: `prek run`
4. Push: `git push -u origin feat/my-feature`
5. `gh pr create --head feat/my-feature --title "..." --body "..."`
```

- [ ] **Step 2: Remove jj-specific gotchas**

In the `## Gotchas` section:
- Delete the line: `- Git hooks do NOT fire under jj — run \`prek run\` explicitly before pushing`
- Delete the line: `- The \`/jj\` skill handles commit/push/PR workflows — use it instead of raw jj commands when available`

- [ ] **Step 3: Verify with grep**

Run:
```bash
grep -n "jj " AGENTS.md || echo "No jj references found"
```
Expected: "No jj references found" (or only matches inside words like "object" if any).

---

### Task 2: Update CONTRIBUTING.md for git

**Files:**
- Modify: `CONTRIBUTING.md`

- [ ] **Step 1: Replace jj setup with git**

Change:
```markdown
- **jj** (recommended) — [jj-vcs.github.io](https://jj-vcs.github.io/jj/latest/install-and-setup/); the repo uses jj in colocated mode
```
To:
```markdown
- **git** — [git-scm.com](https://git-scm.com/); standard git workflow
```

- [ ] **Step 2: Replace DCO sign-off instructions**

Change:
```markdown
Add it with `jj describe`, including the sign-off line in the message:

```
jj describe -m "$(cat <<'EOF'
Signed-off-by: Your Name <your.email@example.com>
EOF
)"
```
```
To:
```markdown
Add it with `git commit`, including the sign-off line in the message:

```bash
git commit -m "Your commit message

Signed-off-by: Your Name <your.email@example.com>"
```
```

- [ ] **Step 3: Replace PR workflow steps**

Change step 1 from:
```markdown
1. Branch from `main` (`jj new main`)
```
To:
```markdown
1. Branch from `main` (`git switch -c feat/name main`)
```

Change step 4 from:
```markdown
4. Address review feedback; use `jj squash` to fold fixups into parent, or `jj rebase -d main` to rebase onto latest main before merge
```
To:
```markdown
4. Address review feedback; use `git commit --amend` to update the last commit, or `git rebase main` to rebase onto latest main before merge
```

- [ ] **Step 4: Verify with grep**

Run:
```bash
grep -n "jj" CONTRIBUTING.md || echo "No jj references found"
```
Expected: "No jj references found"

---

### Task 3: Update bump-version.sh to use git

**Files:**
- Modify: `scripts/bump-version.sh:181-183`

- [ ] **Step 1: Replace jj commit logic with git**

Current code:
```bash
  if command -v jj >/dev/null 2>&1 && [[ -d "$REPO_ROOT/.jj" ]]; then
    jj --repository "$REPO_ROOT" describe -m "chore: bump version to v$VERSION"
    jj --repository "$REPO_ROOT" new
```

Replace with:
```bash
  if command -v git >/dev/null 2>&1 && [[ -d "$REPO_ROOT/.git" ]]; then
    git -C "$REPO_ROOT" commit -am "chore: bump version to v$VERSION"
    git -C "$REPO_ROOT" tag "v$VERSION"
```

- [ ] **Step 2: Verify script syntax**

Run:
```bash
bash -n scripts/bump-version.sh
```
Expected: No output (success).

---

### Task 4: Update release skill

**Files:**
- Modify: `.claude/skills/release/SKILL.md`

- [ ] **Step 1: Replace jj references with git commands**

Find and replace the following:

1. Line ~41: Change description from "Uses jj if available, git otherwise" to "Uses git".

2. Lines ~59-60:
```
jj bookmark set release/vX.Y.Z -r @-
jj git push --bookmark release/vX.Y.Z --allow-new
```
Replace with:
```bash
git switch -c release/vX.Y.Z
git push -u origin release/vX.Y.Z
```

3. Line ~71:
```
$(jj log -r 'main..@-' --no-graph --template 'description' | head -20)
```
Replace with:
```bash
$(git log main..HEAD --oneline | head -20)
```

4. Lines ~96-97:
```
jj bookmark set vX.Y.Z-rc.1 -r @
jj git push --bookmark vX.Y.Z-rc.1
```
Replace with:
```bash
git switch -c vX.Y.Z-rc.1
git push -u origin vX.Y.Z-rc.1
```

5. Line ~143:
```
jj git push
```
Replace with:
```bash
git push
```

6. Lines ~151-152:
```
jj bookmark set chore/dev-bump-vX.Y.(Z+1) -r @-
jj git push --bookmark chore/dev-bump-vX.Y.(Z+1) --allow-new
```
Replace with:
```bash
git switch -c chore/dev-bump-vX.Y.(Z+1)
git push -u origin chore/dev-bump-vX.Y.(Z+1)
```

- [ ] **Step 2: Verify with grep**

Run:
```bash
grep -n "jj" .claude/skills/release/SKILL.md || echo "No jj references found"
```
Expected: "No jj references found"

---

### Task 5: Update docs/RELEASING.md

**Files:**
- Modify: `docs/RELEASING.md`

- [ ] **Step 1: Replace jj bookmark and push commands**

Lines 27-28:
```
jj bookmark set release/vX.Y.Z -r @-
jj git push --bookmark release/vX.Y.Z --allow-new
```
Replace with:
```bash
git switch -c release/vX.Y.Z
git push -u origin release/vX.Y.Z
```

Line 64:
```
jj git push
```
Replace with:
```bash
git push
```

- [ ] **Step 2: Verify with grep**

Run:
```bash
grep -n "jj" docs/RELEASING.md || echo "No jj references found"
```
Expected: "No jj references found"

---

### Task 6: Update execution-roadmap.md

**Files:**
- Modify: `docs/specs/execution-roadmap.md`

- [ ] **Step 1: Replace jj workflow description**

Line ~504: Change:
```
This repo is jj-colocated. Use jj bookmarks for all PR branches.
```
To:
```
This repo uses standard git. Use git branches for all PRs.
```

- [ ] **Step 2: Replace jj rebase commands with git rebase**

Lines ~532-538: Replace each `jj rebase -b <branch> -d main` with `git rebase main` (applied on the respective branch).

For example:
```
jj rebase -b refactor/sqlite-extraction -d main
```
Replace with:
```bash
git switch refactor/sqlite-extraction && git rebase main
```

Do the same for:
- `refactor/scoring-injection`
- `refactor/keyword-strategy`

- [ ] **Step 3: Verify with grep**

Run:
```bash
grep -n "jj " docs/specs/execution-roadmap.md || echo "No jj references found"
```
Expected: "No jj references found"

---

### Task 7: Update plugin scripts

**Files:**
- Modify: `plugin/scripts/pre-compact.sh:43-46`
- Modify: `plugin/dev/scripts/pre-compact.sh:43-46`
- Modify: `plugin/scripts/commit-capture.sh`
- Modify: `plugin/dev/scripts/commit-capture.sh`

- [ ] **Step 1: Rewrite VCS detection in pre-compact.sh (both copies)**

Current logic (lines 43-46):
```bash
if [ -d "$CWD" ] && command -v jj >/dev/null 2>&1 && (cd "$CWD" && jj root >/dev/null 2>&1); then
  VCS_STATE=$(cd "$CWD" && jj log --no-graph -r '@' -T 'change_id.shortest(8) ++ " " ++ description.first_line()' 2>/dev/null) || VCS_STATE=""
elif [ -d "$CWD" ] && command -v jj >/dev/null 2>&1; then
  VCS_STATE=$(cd "$CWD" && jj log --oneline -1 2>/dev/null) || VCS_STATE=""
```

Replace with git-first detection:
```bash
if [ -d "$CWD" ] && command -v git >/dev/null 2>&1 && (cd "$CWD" && git rev-parse --git-dir >/dev/null 2>&1); then
  VCS_STATE=$(cd "$CWD" && git log --oneline -1 2>/dev/null) || VCS_STATE=""
```

Apply to both:
- `plugin/scripts/pre-compact.sh`
- `plugin/dev/scripts/pre-compact.sh`

- [ ] **Step 2: Simplify commit-capture.sh (both copies)**

In `plugin/scripts/commit-capture.sh`:
- Keep the `git commit` detection and parsing logic.
- Remove the `jj commit` / `jj describe` detection branches.
- Remove the jj fallback parsing comment and logic.

In `plugin/dev/scripts/commit-capture.sh`:
- Same changes as above.

- [ ] **Step 3: Verify syntax of all four scripts**

Run:
```bash
for f in plugin/scripts/pre-compact.sh plugin/dev/scripts/pre-compact.sh plugin/scripts/commit-capture.sh plugin/dev/scripts/commit-capture.sh; do
  bash -n "$f" && echo "OK: $f" || echo "FAIL: $f"
done
```
Expected: All four lines print "OK".

---

### Task 8: Light edit on conductor/campaign-audit-remediation.md

**Files:**
- Modify: `conductor/campaign-audit-remediation.md`

- [ ] **Step 1: Replace active workflow references**

Lines ~54-57: Change:
```
1. `jj describe -m "type(scope): description (#issue)"` — frequently during work
...
4. `jj bookmark set <branch> -r @- && jj git push --bookmark <branch> --allow-new`
```
To:
```
1. `git commit -m "type(scope): description (#issue)"` — frequently during work
...
4. `git push -u origin <branch>`
```

- [ ] **Step 2: Verify with grep**

Run:
```bash
grep -n "jj " conductor/campaign-audit-remediation.md || echo "No jj references found"
```
Expected: "No jj references found"

---

### Task 9: Verify and clean up

**Files:**
- Delete: `.claude/skills/jj-vcs-comprehensive/` (if it exists)

- [ ] **Step 1: Remove jj skill directory**

Run:
```bash
rm -rf .claude/skills/jj-vcs-comprehensive/
```

- [ ] **Step 2: Run comprehensive grep across active files**

Run:
```bash
grep -ri "jj " AGENTS.md CONTRIBUTING.md scripts/bump-version.sh docs/RELEASING.md docs/specs/execution-roadmap.md plugin/scripts/ plugin/dev/scripts/ conductor/campaign-audit-remediation.md .claude/skills/release/SKILL.md || echo "No jj references in active files"
```
Expected: "No jj references in active files"

- [ ] **Step 3: Run quality gates**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```
Expected: All pass.

- [ ] **Step 4: Commit all changes**

```bash
git add -A
git commit -m "chore: purge jj references and restore git workflows"
```

---

## Plan Self-Review

**1. Spec coverage:**
- AGENTS.md rewrite → Task 1
- CONTRIBUTING.md rewrite → Task 2
- bump-version.sh → Task 3
- release skill → Task 4
- RELEASING.md → Task 5
- execution-roadmap.md → Task 6
- plugin scripts → Task 7
- conductor/campaign-audit-remediation.md → Task 8
- jj skill removal + final verification → Task 9

All spec requirements covered.

**2. Placeholder scan:** No TBD, TODO, or vague steps. Every step shows exact text or exact commands.

**3. Type consistency:** N/A — this is a docs/scripts task with no types.
