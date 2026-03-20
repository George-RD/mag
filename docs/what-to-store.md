# What to Store

10 system prompt patterns for building useful memory with MAG. Copy-paste these into your AI tool's system prompt or CLAUDE.md to guide what gets stored.

## 1. Project Decisions

> When I make an architectural decision, store it with the rationale. Tag with the project name.

## 2. Debug Patterns

> When I solve a bug, store the root cause and fix. Include the error message so future searches match.

## 3. Client Preferences

> Store client-specific preferences, constraints, and communication style notes. Tag with the client name.

## 4. Recurring Errors

> When I encounter an error I've seen before, store the fix with the exact error text for future retrieval.

## 5. Workflow Conventions

> Store team conventions: naming patterns, branching strategy, review process, deployment steps.

## 6. Meeting Context

> After a meeting, store the key decisions and action items. Use role or team references for search; only include full names when your organisation's policy allows it.

## 7. Code Patterns

> When I establish a pattern (error handling, API structure, test setup), store it as a reusable reference.

## 8. Personal Preferences

> Store my preferred coding style, tool configurations, and communication preferences.

## 9. Learning Notes

> When I learn something new about a tool, library, or concept, store a concise summary.

## 10. Handoff Context

> At the end of a work session, store what I was working on, what's done, and what's next. Tag as "handoff".

---

## What NOT to Store

Do not store API keys, passwords, tokens, or unredacted personal data. MAG stores memories in plaintext SQLite. Treat it like a notebook, not a vault.

## Usage

Add the patterns you want to your system prompt or `CLAUDE.md`. MAG's `memory_store` tool will be invoked by your AI assistant when the pattern matches.

The key to useful memory: be specific in what you store. "Store important things" produces noise. "Store architectural decisions with rationale" produces signal.
