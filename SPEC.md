# empty-status specification

`empty-status` is a closed, deterministic i3bar reactor. The process owns one
state graph, one typed probe algebra, and one serialization boundary. There are
no unit actors, generic effect messages, internal pub/sub channels, or runtime
plugin seams.

## Laws

1. A configured unit is either a valid live slot or an inert red configuration
   slot. One malformed stanza cannot poison its neighbors.
2. A live slot has at most one probe in flight. A click during a probe may mark
   one immediate rerun; repeated clicks cannot create an unbounded queue.
3. Every probe has a unit-specific timeout. Dropping a subprocess probe kills
   its child.
4. Unit state changes only in `Model::apply` and `Model::click`.
5. External data crosses the reducer boundary as a unit-specific `Sample`,
   never as an untyped effect payload.
6. Pango escaping occurs exactly once, when flat `Markup` runs are formatted.
7. i3bar JSON is produced only by the reactor and uses protocol-native field
   types.

## Interaction

`MouseOrbit<L, R>` is the generic product of two cyclic mode axes. Left click
advances `L`, right click advances `R`, and every other button acts as the
identity. The axes need not be binary; for two binary axes the action is
`C₂ × C₂`, the Klein four-group. The generators commute and each is its own
inverse. `cycle!` emits the successor law for a statically closed axis;
`FiniteCycle<T>` supplies a structurally nonempty, configuration-driven axis.
A binary axis paired with either form of `n`-cycle acts as `C₂ × Cₙ`.

Weather instantiates `C₂ × C₃` as
`Horizon { Immediate, Forecast } × Metric { Temperature, RelativeHumidity,
AirQuality }`.
Its request acquires every coordinate-independent fact, so mode clicks publish
a new projection of the cached sample without invalidating or repeating an
in-flight probe.

Quota instantiates the same product as
`Facet { Remaining, Resets } × FiniteCycle<Provider>`. Right click traverses
providers in configuration order; left click changes the projection without
changing source. Every provider returns the same canonical list of limits:
an optional reset window, a unit-bearing remaining quantity, and an optional
reset epoch. Claude supplies 5-hour and 1-week percentages, Codex supplies a
1-week percentage, and OpenRouter supplies an unwindowed U.S.-dollar balance.
The reset projection renders `no reset` when a provider has no window.
Middle click requests fresh data without changing either coordinate.

## Reactor

`src/reactor.rs` owns the ordered `Slot` vector, probe `JoinSet`, task-to-slot
map, stdin click stream, and stdout stream. A live slot alternates between two
phases:

- `Sheathed { due }`: no probe exists; `due` is its next deadline.
- `Cutting`: exactly one probe exists. Each strike carries the slot revision at
  launch; a click that requests fresh data advances the revision.

The reactor scans the small closed slot vector for the earliest deadline. It
selects among that deadline, one probe completion, and one click line. Probe
tasks may complete concurrently across slots, but reducers run serially inside
the reactor. Replies from an obsolete revision are discarded and immediately
repolled, so a mode-changing click cannot flash a stale view. Task IDs are
retained so a cancelled or panicked probe can be attributed, rendered as an
error, and rescheduled without marooning its slot.

The reactor emits an initial loading snapshot and thereafter emits only when a
view changes. Changes arriving within ten milliseconds share one frame, so a
concurrent polling burst does not serialize the same bar once per unit. Blocks
are reversed at the final boundary, so unit stanzas remain ordered from
rightmost to leftmost as in the config.

## Closed algebra

`src/units.rs` declares exhaustive `Unit`, `Request`, and `Reply` enums. Adding a
unit requires extending all three registries; there is deliberately no external
unit fiction.

Each unit module defines:

- `Config`: its strict TOML schema.
- `Model`: durable reducer state.
- `Request`: owned inputs for one probe.
- `Sample`: the typed fact returned by that probe.
- `request`, `apply`, and `click`: the unit transition surface.
- `probe`: the impure adapter that acquires a `Sample`.

`Request::execute` is the sole dispatcher and applies every timeout. Filesystem,
symlink, HTTP, and blocking-thread operations use the narrow `ProbeIo` adapter.
Subprocess protocols remain in the unit that owns their concrete types.

## Resource policy

The reactor itself prevents overlapping polls. Slow or externally expensive
units have canonical cadence floors:

- Weather: 120 seconds.
- Quota: 15 seconds.

Weather concurrently performs one Open-Meteo forecast request for temperature
and 2 m relative humidity and one Open-Meteo air-quality request per probe;
there is no second cache or refresh clock. The feeds fail independently, so an
AQI outage cannot suppress temperature or humidity and a weather-feed outage
cannot suppress AQI. Net ping mode executes one `ping -c 1 -W 1` child per poll
and stores only a bounded ring of results.
Quota probes every configured source concurrently. It invokes Codex app-server
directly, reads the Claude cache directly, and calls OpenRouter's authenticated
credits endpoint with a management key read from the configured absolute token
path. Every source has an independent deadline below the unit deadline, so one
stalled source cannot suppress its peers. Left and right mode clicks never
trigger I/O; middle click explicitly refreshes all sources.

The process truncates its single `last.log` on startup. Logging therefore has a
one-session disk bound rather than an append-only lifetime.

The Claude statusline helper consumes at most one 1 MiB JSON object with an
incremental boundary scanner, atomically replaces the shared cache, and prints
a compact quota summary.

## Rendering

`Markup` is a flat sequence of text runs with optional typed `Rgb8` foreground
colors. Composition is concatenation; brackets and delimiters are ordinary
unescaped text. Its formatter escapes Pango metacharacters and emits color spans.

A `View` pairs `Markup` with `Health`. Health maps to a typed border color.
`I3Block` is serialize-only and exactly represents the emitted i3 fields,
including a boolean `separator`.

Weather temperature and AQI share one continuous Base16 cold-to-hot gradient.
AQI 0 occupies the same cyan knot as 45°F; AQI 200 reaches terminal violet, and
higher values remain violet. Relative humidity is rounded to an integral
percentage and rendered in cyan without imposing a health classification.

## Errors

Probe failures are either `TransportError` or a unit-owned message in
`ProbeError::Unit`. Registry mismatches and task failures are reactor invariant
faults. All become explicit error `View`s; no probe failure deletes a slot or
terminates the bar.

## Configuration

The root and every unit payload reject unknown keys. `type` and `poll_interval`
are removed before the remainder is decoded into the selected unit's `Config`,
which makes strict flattened schemas unnecessary. Poll intervals must be finite
and positive. Weather and Quota default to 300 seconds; other units default to
0.333 seconds. Unit constructors validate semantic constraints before a live
slot can exist.

`config.example.toml` is the complete normative schema. Any schema change must
change it in the same patch.
