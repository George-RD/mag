---
node: mag.runtime.memory.domain
---
# mag.runtime.memory.domain contract

Domain types and capability traits are MAG's stable semantic contract. They may
not depend on a concrete storage engine, transport, daemon, or hosted provider.

`memory_core::Pipeline` is a compatibility-sensitive CLI adapter, not the
production composition root. No new product behaviour is added to it. Its
placeholder processor changes stored content by adding `processed: `, so the
selected local runtime must preserve that observable behaviour during caller
migration. Removal or behaviour change requires a separate versioned decision,
failing-first regression coverage, and the compatibility period defined by
`dec.select-local-runtime-composition-root`.

New model roles and memory intelligence enter through narrow runtime/domain ports
and converge on one production behaviour. They must not be implemented in both
the legacy adapter and the unselected substrate.
