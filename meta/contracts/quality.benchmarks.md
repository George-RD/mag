---
node: mag.quality.benchmarks
---
# mag.quality.benchmarks contract

Benchmarks use versioned datasets, disclose methodology, and distinguish
retrieval from end-to-end answer quality. Local-first scorecards include
quality, latency, RAM, disk size, cold start, migration cost, and offline
success.

Model comparisons use the same corpus, queries, code revision, candidate limits,
and scoring method. Multilingual profiles include English, Arabic, and
cross-lingual cases. Fine-tuned models require held-out task-success improvement,
reproducible training inputs, licence and lineage metadata, and regression
comparison against the untuned production baseline.

## Recorded memory-intelligence scorecards

The quality CLI at `benches/memory_intelligence/evaluate.py` validates versioned
recorded-run artifacts; it does not implement another production runtime or
invoke models. Scorecards preserve the supplied profile snapshot, embedding-space
identity, dataset digest, and producing code revision. Metadata is recorded, not
independently authenticated or reinterpreted as a new model-profile contract.

Content labels and complete provenance sets are scored separately. Missing and
invalid cases remain in the denominator. Measured performance is distinguished
from missing observations, and every aggregate discloses its sample count.
Reference-fixture success must never be reported as measured model quality.
The development seed is not a held-out model-promotion benchmark.

## CLI producer capture

`benches/memory_intelligence/capture.py` supervises a trusted external producer;
it does not own extraction, model loading, or memory semantics. Requests contain
only protocol version, task, instruction, and source memories. Expected
annotations, case IDs, dataset identity, and metadata never enter the request.
Producers must not read the annotated dataset through other channels. The fresh
working directory and POSIX process group are not a security sandbox.

Every case produces one recorded attempt, including timeouts, nonzero exits,
invalid JSON, and exceeded input/output limits. No retries, output repair, or
favorable-case filtering occur. A same-group descendant must not outlive cleanup.
The scorer's existing strict JSON, validation, and atomic artifact helpers are
shared rather than reimplemented.

Observed latency includes per-case process startup, I/O, parsing, and cleanup;
it is not model-only inference latency. Tokens, model load time, and peak RAM
are not measured by this bridge. Missing observations remain null. Externally
supplied profile/resource metadata remains explicitly unauthenticated.

The MAG producer calls the selected CLI-first `LocalMemoryRuntime` through
`mag intelligence-produce`; it must not add independent Python or MCP memory
semantics. Fixture tests do not constitute a measured local-model baseline.

## Owned local-model baseline

`local_baseline.py` supervises a trusted caller-supplied local server and invokes
that MAG CLI through the existing answer-blind bridge. A private model copy must
match the pinned SHA-256 before any executable starts. The original producer
snapshot remains unchanged alongside separate verified-file evidence. Observed
binary hashes and declared source revisions are not source-to-binary attestation
or proof against a hostile server. Inherited `LLAMA_*` settings are excluded.

Server readiness includes initialization and warmup; it is not isolated model
load time or a cold-cache measurement. Observed Linux server VmHWM excludes the
producer, child processes, GPU, and total host RAM. Missing observations remain
null. Readiness probes and producer execution have bounded whole-process
deadlines; every case attempt is retained and owned process groups are cleaned
up on success and failure. Both readiness endpoints share one startup deadline;
the per-probe allowance covers interpreter startup and the HTTP exchange. Output
must not alias any input.

The opt-in workflow pins the model revision, checksum, server source commit,
and every remote action to an immutable commit SHA. Existing weekly Dependabot
updates maintain the action pins. This is not a hermetic-build guarantee: runner
images and the stable Rust toolchain can still change. It records producing code revision, raw attempts, separate scorecard, build
context, and hardware. Ordinary PR CI uses only fixtures, without downloading a
model. A successful workflow or a development-seed score does not promote a
model or establish held-out quality.

## HTTP request diagnostics

`request_diagnostic.py` observes the selected CLI's actual HTTP bodies against
an owned, bounded loopback recorder. It does not forward, infer, reconstruct
prompts in Python, or score the labelled placeholder response. The existing
answer-blind request helper, subprocess supervisor and artifact helpers are
shared. Header values are excluded; source text in bodies is intentionally
retained and requires an appropriate dataset-handling decision.

Each case retains its first complete body's exact bytes, digest and strict JSON
interpretation, including failures after capture. Missing, malformed, duplicate
or timed-out requests remain visible. Header/body reads have an absolute deadline
and size limits; input/output aliases are refused. Trusted binaries and an
isolated home are not a hostile-process sandbox. Observed binary hashes do not
attest the caller-declared source revision.

The diagnostic artifact must remain distinct from generated-output scorecards.
Server-rendered templates are null until actually observed and linked to server
provenance. A passing wire-contract fixture or real-CLI inspection is not model
quality evidence and cannot qualify production ingestion.

## Server-template replay

`template_diagnostic.py` consumes the separate HTTP request artifact. It validates
all source identities and available body digests before HTTP, then sends each
successful request's exact bytes once to `/apply-template` on a trusted literal
loopback server. No message reconstruction, generation, retry, output repair or
scoring occurs. Failed source attempts and server/property failures remain
visible. Redirects and environment proxies are disabled; complete responses or
explicitly truncated prefixes retain byte digests. Whole-exchange deadlines use
the existing process supervisor, not only per-read socket timeouts.

The caller owns server lifecycle and verification. Supplied server context is
preserved but not authenticated by this client. Observed template text, its
hash and source linkage are separate from actual generation evidence;
`generation_prompt` stays null. A source audit showing shared server parsing does
not attest a historical generation prompt or binary equivalence. Header exclusion
does not redact source/template text or supplied context. Input/output aliases
are refused and prior request/model archives remain unchanged.

The local baseline runner accepts an explicit `--json-schema` comparison arm.
It passes that mode to both CLI description and every answer-blind attempt,
rejects a contradictory description before server launch, and preserves the
configured schema beside existing verified-artifact evidence. Request semantics
change only by adding native output constraints; prompt, scorer and dataset do
not change. See `benches/memory_intelligence/SCHEMA_COMPARISON.md`. A schema-valid
completion is not evidence of correct extraction or production readiness.
