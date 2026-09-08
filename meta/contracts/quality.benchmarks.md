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
