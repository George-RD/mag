---
node: mag.runtime.memory.domain
---
# mag.runtime.memory.domain contract

Domain types and traits are the stable semantic contract of MAG. They may not
depend on a concrete storage engine, transport, or hosted provider.

`memory_core::Pipeline` is currently a compatibility-sensitive CLI adapter, not
the universal production composition root. Its placeholder processor changes
stored content by adding `processed: `, so replacement requires explicit
behavioural tests and a migration decision. New intelligence must not be added
to both this adapter and substrate; legacy and replacement orchestration surfaces
must converge on one accepted behaviour.
