# Connector Wave DG Review — Round 1

> Reviewer: DG subagent
> Date: 2026-03-31
> Subject: `docs/strongholds/connector-wave-plan.md`
> Verdict: **REVISE**

---

## Summary

The plan has a correct high-level instinct (other tools deserve richer integration than raw MCP) and the `ContentTier` simplification over `ToolProfile` is sound. However it contains one critical factual error that invalidates the entire Wave A implementation as written, two medium-severity design problems, and several gaps that will cause silent failures at install time. The plan must be revised before implementation begins.

---

## Findings

### F1 — CRITICAL: Codex CLI does not have a skills directory

**Severity: HIGH**

**Claim in plan:** Codex CLI supports `~/.config/codex/skills/<name>/SKILL.md` and `install_skills()` will write four SKILL.md files there.

**Evidence:** Codex CLI's published documentation (README, cli/docs/) describes exactly one mechanism for custom agent guidance: `AGENTS.md` files. Three locations are searched:
- `~/.codex/AGENTS.md` — global personal guidance
- `AGENTS.md` at repo root
- `AGENTS.md` in current working directory

There is no `skills/` directory, no SKILL.md format, no hooks.json, and no lifecycle event system. The plan's note "Codex `hooks.json` support is speculative" understates the problem: the entire skill mechanism is speculative, not just hooks. The plan treats skills as confirmed and hooks as speculative; the correct state is that neither exists.

**Impact:** All of Wave A Task A2 (4 Codex SKILL.md files) and roughly half of A3 (the Codex branch of `install_skills()`) would write files to a path that Codex never reads. Users would see no error and gain no benefit.

**Fix:** Replace the Codex `ContentTier::Skills` assignment with `ContentTier::Mcp` for now. If Codex guidance is desired, the correct path is a single file at `~/.codex/AGENTS.md`. Update the asset plan: instead of 4 SKILL.md files, produce one `connectors/codex/AGENTS.md` with a condensed version of the four skill descriptions. Wire this through a new `install_agents_md()` function, or add a separate `AgentsMd` tier variant to `ContentTier`. Document the path as `~/.codex/AGENTS.md`, not a skills directory.

---

### F2 — HIGH: OpenCode SKILL.md frontmatter is incompatible with the proposed asset content

**Severity: HIGH**

**Claim in plan:** OpenCode skill adaptation is "minimal" and "identical to Codex minus one frontmatter field." The example frontmatter in the existing Claude Code SKILL.md includes `user-invocable: true` and `allowed-tools: [Bash]`.

**Evidence:** OpenCode's skills documentation specifies that SKILL.md frontmatter supports exactly these fields: `name` (required), `description` (required), `license` (optional), `compatibility` (optional), `metadata` (optional, string-to-string map). There is no `user-invocable` field and no `allowed-tools` field. OpenCode's skills system does not expose a concept of which tools the skill may use; permission is governed separately via `permission.skill` patterns.

Additionally, the invocation syntax may differ. The existing Claude Code skills call `mag hook store` and `mag hook search`, which reference the `mag hook` command. The parent plan (R2 H1 finding) established that `mag hook` does not exist as a CLI command and all current hook scripts calling it are NOPs. If OpenCode skills are written to call `mag hook store`, they will silently fail for the same reason. The plan does note that skills use "CLI commands, not MCP tool names" as a risk mitigation, but does not address whether the specific CLI calls in the skills are valid.

**Impact:** Installing OpenCode skills with Claude Code frontmatter fields will either be silently ignored (unrecognized fields stripped) or cause parse errors depending on OpenCode's strictness. The `mag hook` calls within the skill body will fail at runtime.

**Fix:**
1. Strip `user-invocable` and `allowed-tools` from OpenCode SKILL.md frontmatter. OpenCode only requires `name` and `description`.
2. Rewrite skill body CLI calls to use the commands confirmed working in the parent plan: `mag process` for store operations, `mag search` (or equivalent) for recall. Audit each skill's Bash commands against the current CLI before shipping.
3. Add a new task to Wave A: "Audit all CLI commands in skill bodies against current `mag` CLI before writing asset files."

---

### F3 — MEDIUM: Windsurf rules path is wrong

**Severity: MEDIUM**

**Claim in plan:** Windsurf rules file is a single `mag-memory.md` placed somewhere (path unspecified in the plan, but the asset directory shows `windsurf/mag-memory.md`).

**Evidence:** Windsurf's rules system uses:
- Global rules: `~/.codeium/windsurf/memories/global_rules.md` — a single file, always active, 6,000-character limit.
- Workspace rules: `.windsurf/rules/*.md` — per-project markdown files in this directory.

There is no single-file `mag-memory.md` at the global level that gets merged alongside other rule files the way the plan implies. The global path is a single overwritable file (`global_rules.md`), not a directory where MAG can drop its own named file. Writing to `global_rules.md` would clobber any user-written global rules.

The plan defers Wave B pending the MCP facade, which is correct. But the path assumption is wrong and will cause data loss if implemented as-is.

**Impact:** `install_rules()` for Windsurf would either write to the wrong path (no effect) or clobber the user's `global_rules.md` (destructive).

**Fix:** For workspace rules, use `.windsurf/rules/mag-memory.md` relative to the project root. For global rules, either skip Windsurf global install entirely or append to `~/.codeium/windsurf/memories/global_rules.md` with a sentinel comment (risky — can exceed the 6,000-character cap). The safest option: Windsurf rules are project-scoped only, and `install_rules()` requires `project_root` to be `Some`. Document this constraint.

Since Wave B is deferred, add a note to the Windsurf task that the rules target is `.windsurf/rules/mag-memory.md`, not a file at the global path.

---

### F4 — MEDIUM: Cursor rules path and format are confirmed but the plan omits them

**Severity: MEDIUM**

**Claim in plan:** Cursor gets `cursor/mag-memory.mdc` (asset directory). No path for install destination is stated.

**Evidence:** Cursor rules live in `.cursor/rules/*.mdc` (project-scoped). Frontmatter supports `description`, `alwaysApply` (bool), and `globs`. There is no global rules directory — Cursor rules are always project-scoped.

The plan is silent on whether `install_rules()` for Cursor targets the project root or some global location. Since Cursor has no global rules directory, the function must require `project_root` to be `Some` for Cursor, same as Windsurf.

The `.mdc` format assumption is correct. The frontmatter fields are confirmed. No blocking issue, but the plan's omission of the install destination is a gap that will surface as a design decision during implementation and could cause confusion.

**Impact:** Low if implementation defaults to project-scoped. Medium if implementation attempts a global path that doesn't exist.

**Fix:** Explicitly state in the plan: "Cursor rules install to `{project_root}/.cursor/rules/mag-memory.mdc`. No global path exists. `install_rules()` skips Cursor if `project_root` is `None`." Add this to the Wave B section.

---

### F5 — MEDIUM: `ContentTier` enum assigns `GeminiCli` to `Skills` with "TBD"

**Severity: MEDIUM**

**Claim in plan:**
```rust
Self::Codex | Self::GeminiCli => ContentTier::Skills, // GeminiCli TBD
```

**Evidence:** The `// GeminiCli TBD` comment acknowledges this is unverified. Gemini CLI (`gemini-cli`) uses `AGENTS.md` files at project and global scope (`.gemini/AGENTS.md`, `~/.gemini/AGENTS.md`), not a skills directory. Assigning it to `ContentTier::Skills` in production code will route it through `install_skills()` which will write files to a path that doesn't exist.

**Impact:** `mag setup` with Gemini CLI detected would attempt to install skill files to an unverified path, producing silently unused files or a confusing error.

**Fix:** Set `GeminiCli => ContentTier::Mcp` until the correct path is confirmed. If Gemini CLI does support an AGENTS.md-like mechanism, add it to the same `install_agents_md()` function proposed in F1. Remove the `// TBD` pattern from production match arms; speculative assignments should stay in the design doc, not in code.

---

### F6 — MEDIUM: `install_hooks()` is in the function signature but Codex hooks are acknowledged as "speculative"

**Severity: MEDIUM**

**Claim in plan:** `install_hooks(tool: AiTool, home: &Path) -> Result<bool>` is a new function, and "if Codex doesn't support lifecycle hooks yet, skip `install_hooks()` and ship skills-only."

**Evidence:** As established in F1, Codex has no hooks.json. There is no evidence any non-Claude-Code tool in the current AiTool set supports lifecycle hooks. The function signature exists for a use case with zero confirmed targets.

**Impact:** Dead code from day one. The function signature adds maintenance surface with no benefit.

**Fix:** Remove `install_hooks()` from the Wave A scope entirely. If a future tool confirms hook support, add the function then. This also reduces Wave A effort from ~4 hours (A3) to ~3 hours and eliminates the risk of accidentally attempting to install hooks on tools that don't support them.

---

### F7 — LOW: `memory-store` skill body calls `mag hook store` — this is the broken command

**Severity: LOW**

**Claim in plan:** "Adaptation from existing Claude Code skills is minimal." The existing `plugin/skills/memory-store/SKILL.md` contains:
```bash
mag hook store "Brief title..." \
  --project PROJECT \
  --event-type TYPE \
  ...
```

**Evidence:** The parent plan's DG Round 2 finding H1 explicitly states: "`mag hook` does not exist as a CLI command — scripts were silent NOPs." This applies equally to skill invocations. Any adapted skill that copies the `mag hook store` call body will fail silently in any tool, including OpenCode.

**Impact:** The four skills being adapted as the "easy copy+tweak" will carry a broken command if copied naively. The adapted skills will appear to work (no error surfaced to the user) but will store nothing.

**Fix:** Before writing any connector skill files, first fix the source skills in `plugin/skills/` to use the corrected CLI commands established in the parent plan. Then adapt from the fixed source. Add a gate to Wave A: "Fix source skills in `plugin/skills/` before creating adapted copies." This should be tracked as a dependency on the parent plan's Wave 1 hooks work.

---

### F8 — LOW: No uninstall path for installed skills/rules

**Severity: LOW**

**Claim in plan:** `mag setup --force` re-installs all assets (mentioned in risks). The plan says nothing about removal.

**Evidence:** `setup.rs` has `run_uninstall()` support for MCP config removal. There is no equivalent for skill directories or rules files. If a user runs `mag uninstall`, the MCP entry is removed but skill files remain at `~/.config/opencode/skills/memory-*` or `.cursor/rules/mag-memory.mdc`. Running `mag setup` again after reinstall would re-add MCP but skills may already exist from a prior version.

**Impact:** Stale skills from an old MAG version can survive uninstall, causing version skew between skill content and the binary.

**Fix:** Extend `run_uninstall()` to remove known skill/rules paths, mirroring the install logic. Add a task to Wave A: "Extend `run_uninstall()` to remove Codex/OpenCode skills on uninstall." This adds ~1 hour to A3 effort.

---

### F9 — LOW: Testing plan has no negative-path coverage

**Severity: LOW**

**Claim in plan:** Five test items, all positive-path (creates files, idempotency, integration).

**Evidence:** No tests cover: tool not detected (install_skills skips gracefully), missing home directory, unwritable target directory (permissions), partial install (3 of 4 skills written before error), skill directory already exists as a file rather than a directory.

These are the failure modes most likely to produce confusing user-facing errors on first run on a real system.

**Fix:** Add three test items:
- "install_skills() returns Ok(0) when tool has ContentTier::Mcp (no-op)" — guards against regression if tier routing changes
- "install_skills() returns Err if target directory is unwritable"
- "install_skills() on partial prior install (some files exist, some don't) completes without error"

---

### F10 — LOW: Effort estimates do not account for fixing source skills first

**Severity: LOW**

**Claim in plan:** Wave A total ~1.5 days. A2 (8 SKILL.md files) = 2 hours.

**Evidence:** As established in F7, the source skills in `plugin/skills/` contain broken `mag hook` calls that must be fixed before adaptation. Fixing four source skills and ensuring the CLI commands are correct is not captured in any task estimate. The parent plan's Wave 1 hook rewrite is estimated at 0.25 day for session-start/compact-refresh and 1 day for session-end. Skill body rewrites were listed separately at 0.5 day in the parent plan's Phase 1.

If skill fixes are a prerequisite to connector work, Wave A cannot start until the parent plan's Wave 1 is at least partially complete. This is a sequencing dependency the connector plan does not acknowledge.

**Fix:** Add an explicit prerequisite: "Wave A depends on parent plan Wave 1 item 6 (skill rewrites for scoping conventions)." Adjust the Wave A total estimate to reflect that A2 starts after the source skills are fixed, not in parallel with Wave 1.

---

## Cross-Plan Consistency Check

The connector plan's parent reference is correct (`mag-improvement-plan.md`, Ongoing: Competitive Maintenance, Cross-tool section). The parent plan states:

> "Tier 2 tools (Codex CLI, OpenCode) need skill installation via `install_skills()` in setup.rs."

The connector plan correctly implements this. However, the parent plan also states:

> "Auto-capture hooks only benefit Claude Code users; other tools remain MCP-only."

The connector plan's `install_hooks()` function contradicts this — it attempts hook installation for Codex. F6 above resolves this inconsistency by removing `install_hooks()` from Wave A scope, which brings the two plans back into alignment.

The Wave B deferral condition ("deferred until after MCP facade ships") aligns correctly with the parent plan's sequencing: Wave 2 (MCP facade) precedes rules content that references MCP tool names.

No other conflicts found.

---

## Verified Correct Assumptions

These parts of the plan are accurate and should be preserved:

- `ContentTier` enum approach is well-designed. Adding a method to `AiTool` instead of a separate struct correctly mirrors how `config_format()` and `config_paths_for_tool()` are implemented. No objection.
- OpenCode skill directory `~/.config/opencode/skills/<name>/SKILL.md` is confirmed by OpenCode docs. The project-scoped path `.opencode/skills/<name>/SKILL.md` also exists.
- Cursor `.mdc` file format is confirmed. Frontmatter fields `description`, `alwaysApply`, `globs` are correct.
- Wave B deferral rationale (wait for MCP facade before writing rules that reference tool names) is sound.
- Binary size risk dismissal is correct. Eight ~1KB files add negligible size vs the 134MB ONNX model.
- `include_str!()` for static assets is the correct Rust pattern for this use case.
- `mag setup --force` as the re-install mechanism is correct and consistent with existing setup.rs behavior.

---

## Required Changes Before Implementation

| # | Change | Severity | Blocks |
|---|--------|----------|--------|
| R1 | Replace Codex `ContentTier::Skills` with `ContentTier::Mcp` (or new `AgentsMd` tier). Replace 4 SKILL.md assets with single `~/.codex/AGENTS.md`. | HIGH | Wave A |
| R2 | Strip unsupported frontmatter from OpenCode SKILL.md files (`user-invocable`, `allowed-tools`). | HIGH | Wave A |
| R3 | Fix skill body CLI calls — `mag hook store/search` are broken. Establish dependency on parent plan Wave 1 skill rewrites. | HIGH | Wave A |
| R4 | Set `GeminiCli => ContentTier::Mcp` until confirmed. Remove `// TBD` from production match arms. | MEDIUM | Wave A |
| R5 | Remove `install_hooks()` from Wave A scope. No confirmed hook targets exist. | MEDIUM | Wave A |
| R6 | Document Windsurf rules target as `.windsurf/rules/mag-memory.md` (project-scoped). Remove any assumption of global install. Note 6,000-char limit on global path. | MEDIUM | Wave B |
| R7 | Document Cursor rules target as `{project_root}/.cursor/rules/mag-memory.mdc`. Note no global path exists. | MEDIUM | Wave B |
| R8 | Extend `run_uninstall()` to remove skill/rules files. Add to Wave A scope. | LOW | Wave A |
| R9 | Add three negative-path test cases to Wave A testing. | LOW | Wave A |

---

## Revised Effort Estimate

After applying required changes:

**Wave A revised:** ~1.5 days (unchanged in total, but task composition changes)
- A1: `ContentTier` enum + method, including `AgentsMd` tier if adopted — 2 hr (unchanged)
- A2: 4 OpenCode SKILL.md files (fixed frontmatter) + 1 Codex AGENTS.md — 2 hr (unchanged, different deliverable)
- A3: `install_skills()` for OpenCode + `install_agents_md()` for Codex, extended uninstall, wire into `configure_tools()` — 4 hr (unchanged, no `install_hooks()`)
- A4: Tests including 3 negative-path cases — 4 hr (+1 hr)

Note: Wave A is now gated on parent plan Wave 1 item 6 (skill rewrites). If that work is not done first, add 0.5 day for fixing source skills as a prerequisite.

**Wave B:** Unchanged at ~0.75 day, but install targets are now fully specified.

---

## Verdict: REVISE

Two HIGH findings (Codex has no skills directory; OpenCode frontmatter is incompatible and skill commands are broken) block implementation. The plan cannot be executed as written without producing silently non-functional assets. The fixes are well-scoped — no architectural changes required, just corrected paths, corrected frontmatter, and removal of the `install_hooks()` dead function. Resubmit after incorporating R1–R5.
