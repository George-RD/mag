---
node: mag.runtime.doctor
---
# mag.runtime.doctor contract

Doctor checks report actionable runtime, storage, model, and connector health.
They distinguish a pre-first-use missing model from a failed initialized model,
do not silently claim repair, and keep diagnostics separate from core semantics.
