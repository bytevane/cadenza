# ADR 0001: Project name is Cadenza

## Status

Accepted.

## Context

The project implements a Symphony-style orchestration runtime. It should reflect orchestration, musical structure, controlled agent execution, and extensible components without implying official affiliation with Symphony.

## Decision

Use **Cadenza** as the project name.

## Rationale

A cadenza is a solo passage within a concerto where the performer has room for expressive execution while still belonging to the larger composition. This maps well to Cadenza's design: Codex sessions, AI-generated code, and Wasm plugins can be powerful and expressive, but the Rust host remains the conductor and safety boundary.

## Consequences

Module names should use the `cadenza-*` prefix. The tagline should be:

> Cadenza: a Rust + WebAssembly orchestration runtime for Symphony-style Codex workflows.
