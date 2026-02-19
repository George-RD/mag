# CONDUCTOR GUIDE

## OVERVIEW
`conductor/` is the planning and governance layer: product intent, track plans/specs, and style/workflow rules.

## STRUCTURE
```text
conductor/
├── product.md                 # Product goals and parity objective
├── tracks.md                  # Active track index
├── tracks/<track_id>/         # plan.md + spec.md per initiative
├── code_styleguides/          # rust/general style constraints
├── workflow.md                # Delivery and quality gate process
└── archive/                   # Completed track history
```

## WHERE TO LOOK
| Task | File | Notes |
|---|---|---|
| Confirm feature direction | `conductor/product.md` | Parity with omega-memory is explicit objective |
| Start/continue initiative | `conductor/tracks.md` + track `plan.md` | Track status lifecycle `[ ]` -> `[~]` -> `[x]` |
| Coding constraints | `conductor/code_styleguides/rust.md` | Rust-specific quality rules |
| Process constraints | `conductor/workflow.md` | Test/lint/review gates before completion |

## CONVENTIONS
- Treat `plan.md` as execution source of truth during implementation.
- Keep specs/plans concise and actionable; update statuses as work progresses.
- Reflect merged outcomes in `tracks.md` so track index stays current.

## ANTI-PATTERNS
- Do not mark tasks complete without evidence (tests/lints/review resolution).
- Do not leave stale track statuses after merge.
- Do not diverge implementation from documented spec without updating docs.
