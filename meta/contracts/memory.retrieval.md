---
node: mag.runtime.memory.retrieval
---
# mag.runtime.memory.retrieval contract

Retrieval combines independently useful channels, preserves explainability,
abstains when evidence is weak, and is benchmark-gated. Dense first-stage
retrieval and cross-encoder or late-interaction reranking remain distinct roles.

Rerankers operate over a bounded candidate set by default. A full multi-vector
retrieval index is a separate storage and architecture decision, not an
incidental reranker implementation detail.

Fixed thresholds are provisional and calibration is model-profile-specific.
Changes report recall, precision, abstention, score margin, active-injection task
success, latency, RAM, disk/index growth, and context-budget effects. A new model
is not promoted solely from vendor benchmarks or embedding similarity gains.
