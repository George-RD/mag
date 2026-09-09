# Pinned-server template observation, 9 September 2026

All 14 original #447 request bodies rendered successfully. The observed templates
retain each original system and user message in order, including the Arabic
source, and end with `<|im_start|>assistant\n`. The client sent the original
bytes to `/apply-template` once per case, not to a generation endpoint. No answer,
score, prompt edit, schema constraint or production-enabling decision was made.

## Original archive and provenance

`evidence.zip` is the **unmodified GitHub artifact ZIP**, 26,548 bytes. SHA-256:

```
77603b39db8538420c61355a6726f28ac84a0b38cd1c23a088492062240592dc
```

- Observation run: [34379968205](https://github.com/George-RD/mag/actions/runs/34379968205), successful.
- Artifact: `10115793367`, `memory-intelligence-template-observation`.
- Producing checkout and run head: `278db719e9ca5687a0ba9cc5a739b0219898fca4` (explicit head checkout, not a synthetic PR merge).
- Producing tree: `74b62b25b9d2b0256d42f19c444b0d276f2c9f80`.
- Pinned llama.cpp checkout: `304665fe7ac957df95e3ff8c8c4ffdf92dd6ffa3`.
- Observed server binary SHA-256: `d3714bcaa9b55b5ae6cba41c2fa505262eb39167778c62011fb8b2e270e9689a`.
- Model: LFM2.5-1.2B-Instruct Q4_K_M, 730,895,168 bytes; the private file matched the unchanged pin before startup and after cleanup.

The workflow source, launch context and toolchain are inside the archive. The
measurement uses the baseline's CPU/context/thread/decoding settings; its model
alias is deliberately `mag-request-diagnostic` to preserve the captured bytes.
The source revision and observed binary digest are not source-to-binary
attestation or proof of binary equality with the earlier generation run. This
replay does not rebuild or rerun MAG.

| Member | Bytes | SHA-256 |
| --- | ---: | --- |
| `acquisition-workflow.yml` | 8,271 | `3c78cbb9c3e70cbad224221fa41e6c34dff725084735d0cd0d7c484c9576b7e0` |
| `intelligence-request-diagnostic.json` | 44,986 | `24cafb65c082f5faeb119b6c1f57031b4b2996d1313b942f92bc31d895309f4c` |
| `pin.json` | 572 | `80c7acec0b198f9f1f66459d0bc4418f2be415b0c6af7ef17f8e2f51a0aafada` |
| `server-context.json` | 1,836 | `9852b49d762644a7bfe07280f9e1caf48f974ddd90809113d4d19d4deebcdfaa` |
| `templates.json` | 112,811 | `1dd73d86abd96bfd4a19a46ce6bd39199907ef7bcdb6b65287f69a8cf2de243c` |
| `toolchain.txt` | 392 | `a81c884da255450c643730f50550d8943b2c7808d9ae91ba527cc05e18d2d54c` |

The embedded original request artifact is byte-identical to #447's archive under
`../2026-09-09-http-request-v1/`. That older ZIP has SHA-256
`bdd8447213a0bda23385feeb98426ba9aae017e8363029685ec80a58616eb040`.
Its producing MAG binary, revision and dataset identities are retained separately
from this diagnostic's code/server context; no historical archive was rewritten.

## What the observations establish

The server reported model alias `mag-request-diagnostic`, Q4_K - Medium, one
slot, context 4096, build `b1-304665f`, and a chat template supporting system and
string-content messages. Every `/apply-template` response was HTTP 200, strict
JSON and within the byte limit. No request or response attempt was discarded.
An independent offline test checks archive/request/response/prompt integrity,
message preservation and the absence of quality-score claims.

The shared parser path is audited in
`meta/reviews/memory-intelligence-template-diagnostic.md` (relative to repository
root). These observations weigh against dropped system/user messages in this
replay; they do **not** establish the cause of the failed model baseline. Every
case's `generation_prompt` remains null: this is not a capture of the tokenized
prompt used for earlier generation.

The rendered strings do not include the displayed `<|startoftext|>` BOS token.
That alone is not a missing-token finding: pinned `common/chat.cpp` lines 949-954
can remove BOS/EOS text, and completion tokenization separately adds special
tokens. Token IDs and generation were not observed here. `/props` defaults are
server metadata, not a record of an individual generation's effective settings.

The archive contains synthetic development sources and no model binary, headers
or credentials. Its workflow file is inert evidence, not an installed workflow.
Reproduce the client with the instructions in `../../TEMPLATE_DIAGNOSTIC.md` and
a trusted running server; preserve a new run separately rather than overwriting
this archive. Current production qualification remains blocked on extraction
usefulness, held-out and rule-only comparisons, and local resource budgets.
