# Runtime observation, 10 September 2026

Original run: 34464045279, artifact 10146906915.
Producing source: `66796b3995766ead2429e56ccdc3d4ba2568a1be`.
Producing tree: `3d7c3323587374f9b151fbdf16b465f60bb619dd`.
Command: `cargo run --locked --release --bin memory_runtime_eval -- --json`.
Linux x86_64 hosted runner, four CPUs, shared checksum-verified BGE profile.

`run.json` is copied byte-for-byte from the original artifact. SHA-256:
`3f21b208374e6543287723ca90a1ce9b3e31d316e06a7d3540d002da35a3e7c6`.
The original artifact ZIP digest is
`430a1376e9c25bfb7a524139c11d642938236e957003418e587abc0caa2bbeb0`.

All eight families were observed; 36 seeds produced 34 retained memories.
Entity micro F1 is 7.4%, grouping coverage 0%, supersession F1 50%,
temporal recall 75%, relationship recall 66.7% and question recall 90%.
Lifecycle accuracy and observed source-link integrity are 100% on tiny
denominators. These metrics measure different things and are not averaged.

This is a development corpus, not held-out qualification or generative
extraction. It confirms historical failure examples still deserve work;
it does not compare directly with the separate 14-case extraction scorer.
Model startup includes warmup, not isolated loading; tokens remain null.
Linux VmHWM is a process high-water mark, not total host or GPU memory.
The build command and declared source are recorded, not cryptographically
attested source-to-binary identity. No producing binary digest was captured.
The regression protects artifact integrity, not independent rescoring or
future runtime quality. No favorable rerun replaced this observation.
