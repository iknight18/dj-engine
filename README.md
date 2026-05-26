# DJ Engine

Two-program system that DJs a directory of audio files and streams the live mix to Icecast.

- **`analyzer/`** — Python CLI: scans music, writes one JSON sidecar per track.
- **`engine/`** — Rust binary: reads sidecars, beatmatches + harmonically mixes, streams to Icecast.
- **`schema/sidecar.schema.json`** — the contract between the two halves.
- **`deploy/`** — Kubernetes manifests for Rancher deployment.

See `docs/superpowers/specs/` for the design and `docs/superpowers/plans/` for implementation plans.

## Dev quickstart (filled in over the course of the plans)

TBD — see plan 02-analyzer.md and plan 03-engine.md.
