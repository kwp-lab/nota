# ADR 0013: Allow Configurable Trusted DashScope API Roots

- Status: Accepted
- Date: 2026-09-23
- Last updated: 2026-09-23
- Decision owners: Nota maintainers
- Supersedes: The fixed-China-public-endpoint constraint in
  [ADR 0010](0010-direct-dashscope-file-transcription.md)

## Context

ADR 0010 deliberately fixed the first DashScope integration to
`https://dashscope.aliyuncs.com/api/v1`. Alibaba Cloud Model Studio now
recommends workspace-specific API hosts in the form
`{workspaceId}.{region}.maas.aliyuncs.com`. The shared domain remains compatible,
but it no longer receives new platform features and cannot provide the same
workspace-level network isolation.

Workspace hosts are tenant- and region-specific, so Nota cannot replace the
old constant with one universal new constant. Sending an API key and recording
to an arbitrary user-entered host would weaken the provider trust boundary.

## Decision

Nota continues to connect directly from the Rust backend through the dedicated
DashScope adapter, but the adapter uses the API root stored with the selected
ASR Provider instead of a process-wide constant.

The settings UI makes the DashScope API root editable and keeps the supported
model fixed. The existing shared China endpoint remains the default for
backward compatibility. A configured root must:

- use HTTPS without embedded credentials, query parameters, fragments, or a
  custom port;
- use a supported DashScope shared `aliyuncs.com` host or an Alibaba Cloud
  Model Studio workspace-specific `*.maas.aliyuncs.com` host;
- use the DashScope-native `/api/v1` path.

A bare trusted host and a trusted host ending in `/api` normalize to
`/api/v1`. Upload-policy, task-submission, task-query, and task-cancellation
requests all use that normalized root. Provider-returned upload and result URLs
retain their existing independent Alibaba Cloud HTTPS validation.

## Alternatives Considered

### Keep the shared endpoint fixed

Rejected because it prevents users from adopting Alibaba Cloud's recommended
workspace-specific domain and future platform capabilities.

### Replace the shared endpoint with one workspace endpoint

Rejected because each workspace and region has a different host. Nota has no
product-owned workspace that can represent every user.

### Accept any HTTPS Base URL

Rejected because DashScope requests carry a user API key and references to the
uploaded recording. Restricting the host to Alibaba Cloud preserves the
existing third-party provider trust boundary.

### Ask separately for workspace ID and region

Rejected for now because Alibaba Cloud already supplies a complete API Host in
its console. Accepting that host avoids maintaining a second region matrix in
Nota while still validating the security-sensitive boundary.

## Consequences

Positive:

- existing shared-domain configurations continue to work unchanged;
- users can migrate to their own workspace-specific domain without a database
  migration;
- connection testing validates an unsaved endpoint before it is persisted;
- all DashScope control requests consistently follow the selected Provider.

Costs:

- endpoint validation must track Alibaba Cloud's documented domain families;
- the API Key, workspace, region, and model must remain mutually compatible;
- changing a Provider to a different workspace while one of its remote tasks
  is incomplete may prevent that task from being queried or cancelled.

## Compatibility and Evolution

The Provider table already stores `base_url`, so no schema migration is
required. Existing values normalize to the same URL. Adding another domain
family, protocol path, region-specific model, or editable DashScope model
requires explicit compatibility tests and an update to the provider
specification; it must not silently broaden the trusted-host rule.
