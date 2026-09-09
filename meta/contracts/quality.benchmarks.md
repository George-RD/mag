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
