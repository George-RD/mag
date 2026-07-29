---
node: mag.runtime.setup
---
# mag.runtime.setup contract

Setup owns tool detection, generated configuration, data-path resolution,
installation, and uninstall symmetry. Operations must be idempotent, must not
silently overwrite user-managed content, and must leave an explicit recovery
path when setup is partial.

Generated client configuration must reference an executable transport that the
current binary actually serves. Command transport (`mag serve`) is the current
verified path. Reading daemon metadata or compiling `daemon-http` does not prove
that an HTTP process exists; setup must fail visibly or withhold unsupported
stdio/HTTP configurations until their end-to-end server paths are implemented
and tested.
