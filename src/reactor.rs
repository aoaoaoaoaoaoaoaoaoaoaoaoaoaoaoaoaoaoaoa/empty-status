use std::{
    collections::{BTreeMap, HashMap},
    io::Write,
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    task::{Id, JoinSet},
    time::Instant,
};

use crate::{
    core::{I3Block, I3Click, View},
    probe_io::ProbeIo,
    state::Store,
    units::{Reaction, Reply, Unit},
};

const FRAME_COALESCE: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub struct Slot {
    name: String,
    state_key: Option<String>,
    unit: Option<Unit>,
    view: View,
    cadence: Duration,
    revision: u64,
    pulse: Option<Pulse>,
}

#[derive(Debug, Clone, Copy)]
enum Pulse {
    Sheathed { due: Instant },
    Cutting,
}

#[derive(Debug, Clone, Copy)]
struct Strike {
    slot: usize,
    revision: u64,
}

#[derive(Debug)]
pub struct Reactor {
    slots: Vec<Slot>,
    padding: u8,
    io: Arc<ProbeIo>,
    strikes: JoinSet<Reply>,
    strike_slots: HashMap<Id, Strike>,
    state: Store,
}

impl Slot {
    pub fn live(index: usize, state_key: String, unit: Unit, cadence: Duration) -> Self {
        let name = format!("{}::{index}", unit.name());
        let view = unit.initial_view();
        Self {
            name,
            state_key: Some(state_key),
            unit: Some(unit),
            view,
            cadence,
            revision: 0,
            pulse: Some(Pulse::Sheathed {
                due: Instant::now(),
            }),
        }
    }

    pub fn broken(index: usize, message: impl Into<String>) -> Self {
        Self {
            name: format!("Config::{index}"),
            state_key: None,
            unit: None,
            view: View::error(&format!("unit {index}"), message),
            cadence: Duration::ZERO,
            revision: 0,
            pulse: None,
        }
    }
}

impl Reactor {
    pub fn new(mut slots: Vec<Slot>, padding: u8, io: Arc<ProbeIo>, mut state: Store) -> Self {
        let mut postures = BTreeMap::new();
        for slot in &mut slots {
            let (Some(key), Some(unit)) = (slot.state_key.as_ref(), slot.unit.as_mut()) else {
                continue;
            };
            if let Some(posture) = state.get(key)
                && !unit.restore(posture)
            {
                tracing::warn!(slot = %slot.name, "invalid saved posture; using default");
            }
            if let Some(posture) = unit.posture() {
                let _ = postures.insert(key.clone(), posture);
            }
        }
        state.reconcile(postures);
        Self {
            slots,
            padding,
            io,
            strikes: JoinSet::new(),
            strike_slots: HashMap::new(),
            state,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        let mut output = std::io::stdout().lock();
        writeln!(output, r#"{{"version":1,"click_events":true}}"#)?;
        writeln!(output, "[")?;
        write!(output, "[]")?;
        self.publish(&mut output)?;

        let mut clicks = BufReader::new(tokio::io::stdin()).lines();
        let mut clicks_open = true;
        let mut frame_due = None;
        self.unsheathe_due();

        loop {
            let deadline = match (self.next_deadline(), frame_due) {
                (Some(probe), Some(frame)) => probe.min(frame),
                (Some(probe), None) => probe,
                (None, Some(frame)) => frame,
                (None, None) => Instant::now() + Duration::from_hours(24),
            };
            let changed = tokio::select! {
                completion = self.strikes.join_next_with_id(), if !self.strikes.is_empty() => {
                    match completion {
                        Some(Ok((id, reply))) => {
                            if let Some(strike) = self.strike_slots.remove(&id) {
                                self.finish(strike, reply)
                            } else {
                                tracing::error!(?id, "probe completion has no slot");
                                false
                            }
                        }
                        Some(Err(error)) => {
                            let id = error.id();
                            if let Some(strike) = self.strike_slots.remove(&id) {
                                self.fail(strike, &error.to_string())
                            } else {
                                tracing::error!(%error, "failed probe task has no slot");
                                false
                            }
                        }
                        None => false
                    }
                }
                line = clicks.next_line(), if clicks_open => {
                    if let Some(line) = line? {
                        decode_click(&line).is_some_and(|click| self.click(click))
                    } else {
                        clicks_open = false;
                        false
                    }
                }
                () = tokio::time::sleep_until(deadline) => false
            };
            self.unsheathe_due();
            if changed && frame_due.is_none() {
                frame_due = Some(Instant::now() + FRAME_COALESCE);
            }
            if frame_due.is_some_and(|due| due <= Instant::now()) {
                self.publish(&mut output)?;
                frame_due = None;
            }
        }
    }

    fn unsheathe_due(&mut self) {
        let now = Instant::now();
        for (slot_index, slot) in self.slots.iter_mut().enumerate() {
            let Some(Pulse::Sheathed { due }) = slot.pulse else {
                continue;
            };
            if due > now {
                continue;
            }
            let Some(unit) = slot.unit.as_ref() else {
                continue;
            };
            let request = unit.request();
            slot.pulse = Some(Pulse::Cutting);
            let io = Arc::clone(&self.io);
            let strike = self.strikes.spawn(request.execute(io));
            let _ = self.strike_slots.insert(
                strike.id(),
                Strike {
                    slot: slot_index,
                    revision: slot.revision,
                },
            );
        }
    }

    fn finish(&mut self, strike: Strike, reply: Reply) -> bool {
        let Some(slot) = self.slots.get_mut(strike.slot) else {
            tracing::error!(slot = strike.slot, "completion named an absent slot");
            return false;
        };
        if strike.revision != slot.revision {
            slot.pulse = Some(Pulse::Sheathed {
                due: Instant::now(),
            });
            return false;
        }
        let Some(unit) = slot.unit.as_mut() else {
            tracing::error!(slot = strike.slot, "completion named a broken slot");
            return false;
        };
        let view = unit
            .apply(reply)
            .unwrap_or_else(|message| View::error(&slot.name, message));
        let changed = slot.view != view;
        slot.view = view;
        slot.pulse = Some(Pulse::Sheathed {
            due: Instant::now() + slot.cadence,
        });
        changed
    }

    fn fail(&mut self, strike: Strike, error: &str) -> bool {
        let Some(slot) = self.slots.get_mut(strike.slot) else {
            tracing::error!(slot = strike.slot, "failed probe named an absent slot");
            return false;
        };
        if strike.revision != slot.revision {
            slot.pulse = Some(Pulse::Sheathed {
                due: Instant::now(),
            });
            return false;
        }
        let view = View::error(&slot.name, format!("probe task failed: {error}"));
        let changed = slot.view != view;
        slot.view = view;
        slot.pulse = Some(Pulse::Sheathed {
            due: Instant::now() + slot.cadence,
        });
        changed
    }

    fn click(&mut self, click: I3Click) -> bool {
        let Some(index) = self.slots.iter().position(|slot| slot.name == click.name) else {
            return false;
        };
        let (changed, posture) = {
            let slot = &mut self.slots[index];
            let Some(unit) = slot.unit.as_mut() else {
                return false;
            };
            let before = unit.posture();
            let reaction = unit.click(click.button());
            let after = unit.posture();
            let posture = (before != after).then_some(after).flatten();
            let changed = match reaction {
                Reaction::Inert => false,
                Reaction::Publish(view) => {
                    let changed = slot.view != view;
                    slot.view = view;
                    changed
                }
                Reaction::Refresh => {
                    slot.revision = slot.revision.wrapping_add(1);
                    slot.pulse = slot.pulse.map(|pulse| match pulse {
                        Pulse::Sheathed { .. } => Pulse::Sheathed {
                            due: Instant::now(),
                        },
                        Pulse::Cutting => Pulse::Cutting,
                    });
                    false
                }
            };
            (changed, posture)
        };
        if let (Some(key), Some(posture)) = (self.slots[index].state_key.clone(), posture) {
            self.state.record(key, posture);
        }
        changed
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.slots
            .iter()
            .filter_map(|slot| match slot.pulse {
                Some(Pulse::Sheathed { due }) => Some(due),
                Some(Pulse::Cutting) | None => None,
            })
            .min()
    }

    fn publish(&self, output: &mut impl Write) -> Result<()> {
        let blocks = self
            .slots
            .iter()
            .rev()
            .map(|slot| I3Block::new(&slot.name, self.padding, &slot.view))
            .collect::<Vec<_>>();
        writeln!(output, ",")?;
        serde_json::to_writer(&mut *output, &blocks)?;
        output.flush()?;
        Ok(())
    }
}

fn decode_click(line: &str) -> Option<I3Click> {
    let line = line.trim();
    let candidate = line.strip_prefix(',').unwrap_or(line);
    let candidate = candidate.strip_suffix(',').unwrap_or(candidate).trim();
    if candidate.is_empty() || candidate == "[" {
        return None;
    }
    match serde_json::from_str(candidate) {
        Ok(click) => Some(click),
        Err(error) => {
            tracing::warn!(%error, "discarding malformed i3 click");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use super::{Pulse, Reactor, Slot, Strike, decode_click};
    use crate::{
        core::I3Click,
        probe_io::ProbeIo,
        state::Store,
        units::{Reply, Request, Unit, time},
    };

    fn time_unit() -> Option<Unit> {
        let config = toml::from_str::<time::Config>(r#"format = "%H:%M""#).ok()?;
        time::Model::new(config).ok().map(Unit::Time)
    }

    #[test]
    fn accepts_i3_comma_prefix() {
        let click = decode_click(r#",{"name":"Time::0","button":1}"#);
        assert_eq!(click.map(|click| click.name), Some("Time::0".to_owned()));
    }

    #[test]
    fn click_decapitates_an_in_flight_old_mode() {
        let Some(unit) = time_unit() else { return };
        let io = ProbeIo::new();
        assert!(io.is_ok());
        let Ok(io) = io else { return };
        let mut reactor = Reactor::new(
            vec![Slot::live(
                0,
                "time".to_owned(),
                unit,
                Duration::from_secs(1),
            )],
            0,
            io,
            Store::empty(),
        );
        reactor.slots[0].pulse = Some(Pulse::Cutting);
        assert!(!reactor.click(I3Click {
            name: "Time::0".to_owned(),
            button: 1,
        }));
        assert!(!reactor.finish(
            Strike {
                slot: 0,
                revision: 0,
            },
            Reply::Time(Ok(time::Sample::DateTime("stale".to_owned()))),
        ));
        assert_eq!(reactor.slots[0].revision, 1);
        assert!(matches!(
            reactor.slots[0].pulse,
            Some(Pulse::Sheathed { .. })
        ));
        assert!(!reactor.slots[0].view.body.to_string().contains("stale"));
    }

    #[test]
    fn restart_restores_posture_before_the_first_probe() {
        let root =
            std::env::temp_dir().join(format!("empty-status-reactor-state-{}", std::process::id()));
        let path = root.join("posture.json");
        let _ = fs::remove_dir_all(&root);
        let Some(first_unit) = time_unit() else {
            return;
        };
        let io = ProbeIo::new();
        assert!(io.is_ok());
        let Ok(io) = io else { return };
        let mut first = Reactor::new(
            vec![Slot::live(
                0,
                "time".to_owned(),
                first_unit,
                Duration::from_secs(1),
            )],
            0,
            io,
            Store::at(path.clone()),
        );
        assert!(!first.click(I3Click {
            name: "Time::0".to_owned(),
            button: 1,
        }));
        assert!(matches!(
            first.slots[0].unit.as_ref().map(Unit::request),
            Some(Request::Time(time::Request::Uptime))
        ));
        drop(first);

        let Some(restored_unit) = time_unit() else {
            return;
        };
        let io = ProbeIo::new();
        assert!(io.is_ok());
        let Ok(io) = io else { return };
        let restored = Reactor::new(
            vec![Slot::live(
                0,
                "time".to_owned(),
                restored_unit,
                Duration::from_secs(1),
            )],
            0,
            io,
            Store::at(path),
        );
        assert!(matches!(
            restored.slots[0].unit.as_ref().map(Unit::request),
            Some(Request::Time(time::Request::Uptime))
        ));
        assert!(fs::remove_dir_all(root).is_ok());
    }
}
