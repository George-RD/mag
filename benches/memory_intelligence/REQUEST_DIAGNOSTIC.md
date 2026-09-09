# Inspect the selected CLI's HTTP requests

Use this before changing a prompt to see what MAG actually sends. The diagnostic
runs `mag intelligence-produce` against an owned loopback recorder. It does not
load a model, forward requests, render a chat template, or score an output.

```bash
cargo build --locked --release --features llm
out=$(mktemp -d)
python3 benches/memory_intelligence/request_diagnostic.py \
  benches/memory_intelligence/dataset.v1.json \
  --mag "$PWD/target/release/mag" --code-revision "$(git rev-parse HEAD)" \
  --output "$out/requests.json"
```

The runtime still constructs both messages and the HTTP provider still constructs
the request. Python supplies the existing answer-blind capture input only:
protocol version, task, instruction and sources. Expected answers, case IDs and
dataset metadata do not reach the producer. Case IDs index observations in the
resulting artifact, not in the HTTP request.

## What the artifact establishes

Each attempt retains the first complete HTTP body as base64, its SHA-256, and
its strict JSON interpretation. Exact request bytes remain available when JSON
parsing fails. Timeouts, missing bodies, duplicate calls, malformed HTTP, stream
quotas and nonzero producer exits remain visible; a later failure does not erase
an already recorded body. Header values are not retained.

The recorder returns `MAG_REQUEST_DIAGNOSTIC_NO_GENERATION`. This placeholder is
checked, discarded, and never saved as a model answer. The artifact deliberately
has `requests`, not the scorer's `results` or `model_profile`. It is not a model
run or a successful quality evaluation. Attempt failures still produce an
artifact and exit zero; configuration, input integrity and file-write failures
exit two. Inspect every attempt's `error`, or run the assertion-based CI test.

The binary's observed SHA-256 is checked before and after execution. The supplied
source revision is a caller claim, not proof that those sources built the binary.
`server_rendered_template` is null because no inference server is contacted.

## Bounds and handling

The recorder binds only to loopback and never forwards. Each case has an absolute
recorder deadline covering headers and body, plus the existing whole-process
producer supervisor. Default limits are ten seconds, one MiB for input/body and
each producer stream, and 16 KiB for headers. Only one Content-Length-delimited
POST to `/v1/chat/completions` is accepted; chunked requests are rejected. This
matches the selected HTTP provider, not a general-purpose HTTP implementation.

Use trusted binaries and a trusted local host. Home and MAG data paths are
isolated per case; the remaining environment is inherited. This is not a hostile
process sandbox or endpoint authentication. **Source text is intentionally stored
in full.** Do not use private memories unless writing that evidence is permitted.
Excluding headers does not redact sensitive content inside a body.

The normal Python matrix tests the recorder with transport fixtures. The release
`llm` job additionally runs every development case through the actual Rust CLI
and archives the request artifact, including failed observations. It asserts
system/user roles, answer-blind user content, configured model/decoding fields,
and absence of `response_format`; it does not reconstruct the prompt in Python.

## Next diagnostic boundary

Use the [server-template replay client](TEMPLATE_DIAGNOSTIC.md) to inspect these
exact bodies against a trusted pinned server's actual renderer. It records
server/model context and rendered outputs without generating answers. The dated
observation archive and source audit link that renderer to the chat-completion
parser, not to a witnessed historical generation prompt. Then consider a
separate, non-repairing constrained-output run.
Keep the original failed baselines, scorer and negative controls unchanged.
Neither wire inspection nor schema compliance replaces held-out, rule-only and
resource-budget comparisons before enabling production ingestion.
