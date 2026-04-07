# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |
| < 0.1   | No        |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Use one of these private channels:

1. **GitHub Security Advisories** (preferred): [Report a vulnerability](https://github.com/George-RD/mag/security/advisories/new)
2. **Email**: security@george-rd.dev

Include: affected version, steps to reproduce, potential impact, and any suggested fix.

### Disclosure Timeline

- **Day 0** — Report received; acknowledgement within 48 hours.
- **Day 7** — Triage complete; severity assessment shared with reporter.
- **Day 30** — Target for patch release (critical/high severity).
- **Day 90** — Maximum embargo. Public disclosure happens at 90 days regardless of patch status.

We follow coordinated disclosure and will credit reporters in the release notes unless anonymity is requested.

## Privacy Model

MAG is **fully local**. There is no cloud component or telemetry:

- Memories are stored in a local SQLite database (`~/.mag/memory.db` by default, overridable via `MAG_DATA_ROOT`).
- Embeddings are computed locally using ONNX models downloaded once to `~/.mag/models/`.
- No data leaves the machine. No analytics, no telemetry, no opt-in/opt-out tracking.

Network activity only occurs on first use (model auto-download from Hugging Face) or when an optional API-based embedder (Voyage, OpenAI) is explicitly configured.

## Scope

The following are **in scope** for vulnerability reports:

| Category | Examples |
| -------- | ------- |
| **Data leakage** | Memory contents readable by other local users; insecure file permissions on the SQLite database |
| **Injection** | SQL injection via MCP tool inputs; path traversal in import/export; command injection in hook scripts |
| **Auth bypass** | Bearer token bypass in daemon HTTP mode; timing attacks on token comparison |
| **Privilege escalation** | MAG binary or install script elevating privileges unexpectedly |

The following are **out of scope**:

- Vulnerabilities in the host OS or SQLite itself.
- Attacks that require root/admin access on the target machine.
- Social-engineering attacks against the user.
- Theoretical vulnerabilities with no demonstrated impact.

## Data Location & Encryption

All data lives in a single directory (default `~/.mag/`):

| File | Purpose |
| ---- | ------- |
| `~/.mag/memory.db` | SQLite database — all memories, embeddings, metadata |
| `~/.mag/models/` | Cached ONNX model files |

SQLite databases are stored as plaintext. MAG does not implement application-level encryption. Enable full-disk encryption (macOS FileVault, Linux LUKS, Windows BitLocker) if your threat model requires it.
