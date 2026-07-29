# MAG Improvement Plan

> **Historical plan — 2026-03-31.** Metrics, tool counts, priorities, and next actions below are a dated planning snapshot.
> Use the local-first roadmap for sequencing and Cairn todos for current status.

> Stronghold document. Synthesized from 33 memories across 4 sessions + full codebase survey.
> Status: FINAL -- passed 2 rounds of /simplify + /dg
> Date: 2026-03-31
> DG reviews: `improvement-plan-dg-r1.md`, `improvement-plan-dg-r2.md`

---

## Executive Summary

MAG is at v0.1.4 with 90.1% LoCoMo-10 retrieval, 91.2% E2E, 500+ tests, and all issues closed. Retrieval is competitive with AutoMem (90.5%) while being fully local.

The next phase: **make MAG useful without asking the user to do anything.** Storage requires too much manual effort. The goal is "install MAG and benefit naturally."

Two pillars: reduce friction (MCP redesign), then deliver automatic value (preference layer + auto-capture). Competitive moat work is ongoing maintenance, not a separate initiative.

---

## Pillar 1: Reduce Friction (MCP Redesign)

### Problem
16 MCP tools with ~128 schema params = 2-3K extra tokens per API call. MCP-only mode has no hooks, so nothing is automatic.
