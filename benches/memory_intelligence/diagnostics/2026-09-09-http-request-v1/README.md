# Actual CLI HTTP requests — 9 September 2026

All 14 development cases passed the selected release CLI wire-contract check.
This is a request observation, **not a model-quality result**. No model or
inference server was run and `server_rendered_template` remains null.

## Unmodified source artifact

`evidence.zip` is the original 7,289-byte Actions artifact, copied byte-for-byte.
It contains only `intelligence-request-diagnostic.json`, including exact body
bytes, hashes, parsed bodies and metadata. ZIP SHA-256:
`bdd8447213a0bda23385feeb98426ba9aae017e8363029685ec80a58616eb040`.

- Actions run: https://github.com/George-RD/mag/actions/runs/34370777755
- Artifact ID: `10111864753`.
- PR head at measurement: `1e1cc33b0bee73a83ba10ab6b6d89dbfafe40261`.
- Actual checkout: `0b856597af19235a926de46d657aed86f6da4fc8` (synthetic PR merge).
- Producing tree: `bbb20d5e39c52023acb88e873ebf22daf4adce44`.
- Observed MAG binary SHA-256:
  `2a18e4dc4e814c271869b2eb6dd6cab666cfd0c84cdaf9711d83172b5a783f19`.
- Dataset SHA-256:
  `7a001a0aed48bc0e457c155b6ba0780343bb528c661b7473c79dd653a76ce26e`.

The synthetic merge's parents are main `eaecbd39ac31493aae0b07a6195e4eb20ef48d50`
and the measured PR head. Its tree exactly matches that head. The observed
binary hash is not a source-to-binary attestation. This archive precedes its own
import/replay-test commit; later CI artifacts are separate observations.

## What was observed

Every request has system and user messages, `max_tokens=512`, temperature about
0.1, and model alias `mag-request-diagnostic`. The four top-level request fields
are `max_tokens`, `messages`, `model` and `temperature`; `response_format` is
absent. Parsed user content exactly matches the corresponding answer-blind
protocol request. No case IDs, expected annotations or dataset metadata are
injected. The complete prompt-v1 system message is preserved in the archive.

All body digests were independently checked after downloading. The replay test
checks archive integrity and reuses the real-CLI assertions for every case.
No recorded attempt has an error. A labelled placeholder was returned and
then discarded; there are no generated answers, scorecard or model metrics.

Run the integrity/replay check without Rust or a model:

```bash
python3 -m unittest discover -s tests -p test_memory_intelligence_request_diagnostic.py -v
```

Only the new real-CLI observation test is skipped unless `MAG_DIAGNOSTIC_BINARY`
and `MAG_DIAGNOSTIC_REVISION` are supplied. Archive replay always runs.

## Remaining boundary

These bodies were sent to a loopback recorder, not the original model endpoint.
This verifies the selected CLI's current request construction, not the inference
server's tokenization or rendering. Inspect the pinned server renderer before
another prompt change. Retain the failed model baselines and require held-out,
rule-only and resource comparisons before production enablement. See
[diagnostic usage and handling limits](../../REQUEST_DIAGNOSTIC.md).
