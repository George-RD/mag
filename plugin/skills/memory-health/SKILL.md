---
name: memory-health
description: Check MAG health, stats, and run maintenance
user-invocable: true
allowed-tools:
  - Bash
---

# Memory Health

1. Check daemon: `curl -sf http://127.0.0.1:19420/health | jq .`
2. Check stats: `mag stats`
3. If issues: `mag sweep` to clean expired memories
4. Heavy maintenance: `mag maintain compact`
