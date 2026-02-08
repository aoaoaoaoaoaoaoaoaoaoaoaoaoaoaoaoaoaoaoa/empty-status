# Unit Machines: Explicit Scheduling + Typed Health (Tokio-Centric)

## Intent

We want units to become *pure-ish* state machines that:

- **do not** implement their own ad-hoc scheduling/backoff/poll-cadence policies
- **do** own their *bespoke IO* (HTTP/sysfs/process), but only behind a narrow `poll()` boundary
- report a **typed view** (`Markup`) and a **typed health channel** (`Health`) rather than mashing errors into strings

The runtime becomes a single generic driver that squeezes Tokio primitives hard
(`watch`, `JoinSet`, `CancellationToken`, `timeout`, etc.) without reinventing an
async runtime.

This proposal is a vertical slice: unit execution semantics + scheduling + error
health. Rendering DSL work is explicitly out-of-scope.

## Motivation

Today, each unit intermixes:

- IO
- caching/poll cadence
- click behavior
- backoff/error handling
- view formatting

Even with typed `Markup`, this mixes concerns and makes correctness properties
(staleness, backoff, cancellation, timeouts, uniform error policy) implicit.

The goal is to collapse unit logic to:

> `domain state` → `poll decision` → `poll` → `state update` → `view`

…with scheduling/timeout/backoff/cancellation implemented once.

## Non-Goals

- A full effect algebra / bespoke interpreter for all IO.
- Plugin extensibility / typed config revamp.
- Rewriting `Markup` into a semantic render DSL.

## Proposed Architecture

### Core Types

```rust
// coarse channel: UI policy lives in runtime
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health { Ok, Degraded, Error }

#[derive(Debug, Clone)]
pub struct View {
    pub body: crate::render::markup::Markup,
    pub health: Health,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitDecision {
    Idle,
    PollNow,
}

// Decided policy:
// - clicks cancel any in-flight poll
// - ticks may update the view without polling
// - units own business-logic errors; runtime only classifies transport-class failures
// - backoff/staleness is minimal for v1
// - no push streams in v1: polling may yield batches/aggregates; realtime is undesirable
```

### Unit Machine Trait

Each unit becomes a machine over a concrete `State` and a typed `PollOut`.

```rust
pub trait UnitMachine: Send + Sync + std::fmt::Debug + 'static {
    type State: Send + std::fmt::Debug + 'static;
    type PollOut: Send + std::fmt::Debug + 'static;

    fn name(&self) -> &'static str;
    fn init(&self) -> (Self::State, View, UnitDecision);

    fn on_tick(&self, state: Self::State) -> (Self::State, Option<View>, UnitDecision);
    fn on_click(
        &self,
        state: Self::State,
        click: crate::core::ClickEvent,
    ) -> (Self::State, Option<View>, UnitDecision);

    // Bespoke IO lives here.
    fn poll(
        &self,
        state: &Self::State,
    ) -> impl std::future::Future<Output = anyhow::Result<Self::PollOut>> + Send;

    fn on_poll(
        &self,
        state: Self::State,
        out: anyhow::Result<Self::PollOut>,
    ) -> (Self::State, View, UnitDecision);
}
```

Notes:

- `poll(&State)` is read-only; results flow back through `on_poll`.
- `UnitDecision` is intentionally tiny: runtime owns cadence/backoff.
- Ticks/clicks may optionally return `Some(View)` for render-only updates (clocks,
  local computations) without inducing any poll.

### Tokio-Centric Driver

For each unit machine instance, runtime owns:

- `watch::Sender<View>` for latest view
- a click receiver filtered by `i3_name`
- a `CancellationToken` for canceling in-flight poll
- a backoff state (`next_deadline`, `consecutive_failures`)

Chosen topology: **task-per-unit actor + `watch::Sender<View>` + periodic aggregator**.

Each unit runs in a dedicated Tokio task owning its `State`, in-flight poll
handle, cancellation token, and (later) background tasks. The unit task
publishes its latest `View` over a `watch` channel.

**Decided output policy:** output is *pure periodic*.

The output task periodically reads the latest `View`s and emits i3bar JSON at a
global cadence (no reactive flush-on-change in v1). If we later need
interactivity, we may add an explicit "urgent flush" signal as a separate
channel.

Runtime loop uses:

- `tokio::select!` over:
  - `sleep_until(next_deadline)`
  - `click_rx.recv()`
  - in-flight poll completion (a `JoinHandle`)

### Streams: Deferred (Batch-Only Polling)

Status bars prefer stable, rate-limited snapshots. Realtime push streams are
often actively harmful (flicker, CPU wakeups, noisy updates). For v1, we defer
any runtime-managed streams.

Instead, `poll()` may return *batched/aggregated* data computed since the last
poll. If a unit needs incremental input (e.g. `ping`), it may run a background
task feeding an internal buffer; `poll()` drains/aggregates that buffer into a
single `PollOut` snapshot.

Runtime applies uniform policy:

- `timeout(POLL_TIMEOUT, machine.poll(&state))`
- exponential backoff with jitter on failure
- optional cancellation on click-mode changes
- stable "loading" views while polls are in-flight

### Health Policy

Units choose `Health` in `View`.
Runtime maps `Health` to i3bar `border` or other i3bar fields.

Units do **not** decide "red border" or "yellow border" directly.

## Migration Plan

1. Add `src/machine/` module containing `Health`, `View`, `UnitDecision`, `UnitMachine`.
2. Add a parallel driver used by `EmptyStatus` for machine-backed units.
3. Migrate Weather first (best stress test: polling + click mode).
4. Migrate Net next (process + streaming).
5. Convert remaining units incrementally.
6. Delete old `UnitWrapper` runtime loop after all units migrate.

## Design Blockers (must be resolved)

1. **Error taxonomy**: runtime adds transport-class failures (timeout/cancel) but
   does not interpret payload semantics; how does this surface in `on_poll`?
   (e.g. wrap with `MachinePollError::{Timeout, Canceled, Transport(anyhow::Error)}`)
2. **Timeout policy**: global default vs per-unit configurable.
3. **Backoff policy**: minimal v1; define exact curve + cap.
5. **Concurrency topology**: decided: one task per unit + `watch<View>` +
   periodic aggregator.
