# Inspect server-rendered chat templates

`template_diagnostic.py` replays a recorded MAG HTTP request through an already
running trusted local server's `/apply-template` endpoint. It sends the **exact
recorded body bytes**, including model alias and decoding fields. It does not
rebuild messages, add a schema, generate an answer, or call the scorer.

Start with the separate [CLI request capture](REQUEST_DIAGNOSTIC.md). Start your
server on loopback with the same model alias as the captured bodies. Record its
model checksum, source revision, executable checksum, launch arguments and build
context in a JSON object. For the pinned diagnostic, the dated evidence archive
retains that context and the exact acquisition workflow.

```bash
python3 benches/memory_intelligence/template_diagnostic.py requests.json \
  --base-url http://127.0.0.1:8080 \
  --server-context server-context.json \
  --output templates.json
```

The caller starts, verifies and stops the server. This client does not load a
model or authenticate the supplied context. It records that distinction even
when the caller provides detailed hashes. Use a trusted host and server; a
loopback address is not endpoint authentication or a security sandbox.

## What is retained

The separate artifact links to the original request artifact's byte digest,
producing revision, MAG binary digest and dataset digest. It records one `/props`
observation and one template attempt for each source case. Successful source
requests are POSTed once to `/apply-template`, without a retry or any change to
the original body. Source failures remain visible and are not replayed.

Complete HTTP response bodies are retained as base64 and strict JSON, with their
digests. An oversized body retains only the explicitly labelled bounded prefix.
A connection failure or deadline may leave the response null; no missing bytes
are reconstructed. Status, parse and shape errors remain in the artifact. A
properties failure does not hide subsequent case observations.

A successful response's `prompt` and its UTF-8 SHA-256 are **observed template
output**, not a witnessed historical generation prompt. `generation_prompt`
remains null. The source request archive and prior model baselines are unchanged;
this report has no model answers, scores or production-promotion status.

At llama.cpp revision `304665fe7ac957df95e3ff8c8c4ffdf92dd6ffa3`,
`tools/server/server-context.cpp` routes `/apply-template` and chat completions
through `oaicompat_chat_params_parse` using the same server chat parameters.
The former returns `data.at("prompt")`; the latter schedules completion work.
That source correspondence makes the replay useful, but does not prove what an
earlier server binary actually used during generation. The source audit and
observations belong together; neither substitutes for the other.

## Bounds and handling

Only a literal `http://127.0.0.1:PORT` without a path is accepted. Environment
proxies and redirects are disabled. Each exchange has a whole-process deadline
(default ten seconds) as well as a socket timeout, and a body limit (default one
MiB). The process supervisor and strict JSON/atomic-write helpers are shared with
the existing capture harness. These are HTTP exchange limits, not a bound on
reading a caller-supplied artifact file or a hostile-server memory sandbox.

Header values are excluded. **Source and template text are intentionally retained
in full**, as is the caller's supplied context; use approved inputs only. Do not
put secrets in those files. Input/output aliases and altered source artifacts are
rejected. Attempt failures produce an artifact and exit zero; configuration,
input-integrity and write failures exit two. Inspect every error field before
interpreting a run. Fixture tests need no model or network beyond loopback.

Template inspection cannot establish extraction quality. A separately disclosed
constrained-output comparison, held-out cases, rule-only comparison and local
resource-budget gates remain necessary before production ingestion is enabled.
