# Security policy

## Reporting

Report suspected vulnerabilities privately through GitHub's "Report a
vulnerability" flow on the `ferrite-cad` repository. Please do not open a
public issue for an unfixed vulnerability.

Include the affected version, the platform, and a minimal reproducer file
where possible. If the reproducer is a CAD model you cannot share, a
description of the operation sequence is enough to start.

## Threat model

FerriteCAD is a local desktop application. It does not run a server, does not
require an account, and does not upload models. The realistic attack surface is
**untrusted input files**:

- STEP and other exchange files parsed by Open CASCADE;
- native `.fcad` documents produced elsewhere;
- cache sidecars (`.fcad-cache`) accompanying a received document.

The corresponding rules:

- A cache sidecar is never trusted. It is validated against the document's
  UUID and format version, and is discarded rather than repaired on any
  mismatch. Nothing in a sidecar can change the result of a rebuild.
- Unknown object payloads in a document are preserved byte-for-byte and are
  never interpreted.
- A document requiring an unsupported capability opens read-only.
- Parsing untrusted exchange files is the priority candidate for isolation in
  a separate worker process (architecture-decisions.md, "Геометрическое ядро и
  FFI").

## Crash reporting

Crash reporting is opt-in and must never transmit user model data.

## Supply chain

`cargo deny check` runs in CI on every pull request. Third-party C/C++ archives
are pinned by version and checksum. Dependencies under GPL/AGPL are not linked
into the application process.
