# Security Policy

taetype parses untrusted binary font data (TTF, OTF, TTC, WOFF2) supplied by whoever embeds it. A crash, panic, out-of-bounds read/write, or memory-safety issue triggered by a crafted font file is a security issue, not just a bug.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting: open the [Security tab](https://github.com/silly-tae/taetype/security) on this repository and click "Report a vulnerability." This goes directly to the maintainer and stays private until a fix ships – please don't open a public issue for a suspected vulnerability.

Include a minimal reproducing font file (or a description of how to construct one) and what it triggers – panic, OOB access, hang, excessive memory allocation, or incorrect output that could be exploited downstream.

## Scope

In scope:
- Any input that panics the WASM engine (fatal under `panic = "abort"`), causes a Rust-level memory-safety violation, or triggers unbounded allocation/hang from a malformed or adversarial font file
- Any decode/instance/subset/shape path in `src/font/`

Out of scope:
- The rendered content of a font itself (glyph shapes, embedded metadata) – taetype's job is to not crash or corrupt memory while parsing it, not to police what a font contains
- Rasterization – taetype doesn't rasterize; that's downstream of this library

## Supported versions

taetype is pre-1.0. Only the latest published version is supported – please update before reporting.

## Response

This is a solo-maintained project. No fixed SLA, but security reports get priority over feature work.
