# empty-status specification

This document is the authoritative specification for the current system design.
It is written as a timeless, living description of the codebase as it exists.
Any implemented change must update this spec; no historical notes live here.

## Vision

`empty-status` is a deterministic status bar engine that favors strict types,
compositional rendering, and centralized effects. Units are pure planners with
typed state and explicit effects; the runtime owns scheduling, IO execution, and
error framing. The output path is fully typed until the final i3bar serialization.

## Design principles

1. Illegal states are unrepresentable; prefer strict domain types.
2. Units are logic and state; runtime is scheduling and effect execution.
3. Rendering is typed; stringly formatting is prohibited in unit logic.
4. Effects are centralized; units do not perform IO directly.
5. Errors are structured; transport vs unit errors are explicit.
6. External APIs are hostile: enforce conservative rate limits.

## Architecture overview

### Modules

- `src/machine/runtime.rs`: orchestrates unit actors, polling, and i3bar output.
- `src/machine/types.rs`: core types (`UnitMachine`, `View`, `Availability`, errors).
- `src/machine/effects.rs`: effect engine with caching and rate limiting.
- `src/render/markup.rs`: typed markup builder for output.
- `src/config.rs`: config parsing, scheduling policy, and unit wiring.
- `src/units/*`: domain logic and state for each unit (no direct IO).

### Runtime model

Each unit runs as an actor with:

- `init`: produces initial state, view, and decision (`PollNow` or `Idle`).
- `on_tick`: periodic hook for local state; may request a poll.
- `on_click`: handles click events; may request a poll.
- `poll`: performs effectful reads via `EffectEngine` and returns `PollOut`.
- `on_poll_ok`: maps `PollOut` to `Availability`.

The runtime:

- Owns poll scheduling with a minimum global interval.
- Performs pure periodic output; no reactive flush.
- Renders error frames and error messages centrally.

### Effects kernel

All IO is performed through `EffectEngine` and is type-directed.

Effect requests (`EffectReq`):

- `HttpGet`: HTTP fetch with host-level rate limiting and cache freshness.
- `FsRead`: file read with cache freshness.
- `FsListDir`: directory listing with cache freshness.
- `ProcBatch`: persistent subprocess reader with bounded line drain.

Effect outputs (`EffectOut`) are converted via `EffectOut::expect<T>()` to
eliminate stringly downcasts and keep callsites typed.

Rate limiting and caching are enforced in the engine. Units specify policies;
the engine enforces them.

### Rendering

Units return `Markup` rather than raw strings. `Markup` is a typed render tree
that supports composition, brackets, colors, and escaping. The runtime converts
`Markup` to i3bar JSON only at the final output boundary.

### Error model

Errors are split into:

- `TransportError`: IO boundary failures (timeout, HTTP, transport string).
- `PollError::Unit(E)`: unit-specific domain error.

The runtime renders all errors as uniform error frames. Unit-specific wording is
provided by `UnitMachine::render_unit_error`, but framing and color are owned by
the runtime.

## Invariants

- Units never perform direct IO; all external reads go through `EffectEngine`.
- `Markup` is the only rendering payload in units and runtime views.
- Error framing is centralized in the runtime.
- Effect outputs are type-checked at callsites (`expect<T>`).
- Config parsing rejects unknown keys by default.

## Config

Config is TOML with strict schemas:

- Global settings at top-level.
- Units defined in `[[units]]` with `type` and per-unit fields.
- Unknown keys are rejected.

Config drives both unit construction and scheduling (polling interval per unit).

## Extensibility

New units should:

1. Define a `UnitMachine` in `src/machine/units/*`.
2. Define unit logic/state in `src/units/*` with zero direct IO.
3. Declare any effects in `poll` via `EffectEngine`.
4. Render exclusively through `Markup`.
5. Add config schema and wiring in `src/config.rs`.

New effect types should be added to `EffectReq`/`EffectOut` with typed `expect`.
