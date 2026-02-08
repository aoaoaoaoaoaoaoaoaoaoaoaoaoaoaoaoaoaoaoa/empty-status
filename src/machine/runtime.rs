use crate::config::GlobalConfig;
use crate::core::{OutputChunk, RED, YELLOW};
use crate::machine::types::{
    Availability, Health, PollError, TransportError, UnitDecision, UnitMachine, View,
};
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::watch;

pub struct MachineWrapper {
    pub i3_name: String,
    pub handle: usize,
    pub view_rx: watch::Receiver<View>,
}

fn make_chunk(i3_name: &str, padding: i32, view: &View) -> OutputChunk {
    let mut chunk = OutputChunk::new(i3_name, view.body.to_string());
    let pad = " ".repeat(padding.max(0) as usize);
    chunk.full_text = format!("{pad}{}{pad}", chunk.full_text);
    match view.health {
        Health::Ok => {}
        Health::Degraded => chunk.border = YELLOW.to_string(),
        Health::Error => chunk.border = RED.to_string(),
    }
    chunk
}

fn render_poll_error(name: &str, err: &PollError<impl std::fmt::Display>) -> View {
    let name = name.to_ascii_lowercase();
    let (health, body) = match err {
        PollError::Transport(TransportError::Timeout) => (
            Health::Error,
            crate::render::markup::Markup::text(format!("{name}: timeout")).fg(crate::core::RED),
        ),
        PollError::Transport(TransportError::Transport(msg)) => (
            Health::Error,
            crate::render::markup::Markup::text(format!("{name}: {msg}")).fg(crate::core::RED),
        ),
        PollError::Unit(e) => {
            let msg = e.to_string();
            let short = if msg.contains("429") {
                "HTTP 429".to_string()
            } else {
                let mut s = msg;
                s.truncate(80);
                s
            };
            (
                Health::Error,
                crate::render::markup::Markup::text(format!("{name}: {short}"))
                    .fg(crate::core::RED),
            )
        }
    };

    View { body, health }
}

fn render_availability<E: std::fmt::Display>(
    name: &str,
    availability: Availability<crate::render::markup::Markup, PollError<E>>,
) -> View {
    match availability {
        Availability::Loading => View {
            body: crate::render::markup::Markup::text(format!(
                "{} loading",
                name.to_ascii_lowercase()
            ))
            .fg(crate::core::VIOLET),
            health: Health::Degraded,
        },
        Availability::Ready(body) => View::ok(body),
        Availability::Failed(err) => render_poll_error(name, &err),
    }
}

fn i3bar_order(handles: &[usize]) -> Vec<usize> {
    let mut out = handles.to_vec();
    out.reverse();
    out
}

pub async fn run_empty_status_machines(
    mut wrappers: Vec<MachineWrapper>,
    cfg: GlobalConfig,
    _click_tx: broadcast::Sender<crate::core::ClickEvent>,
) {
    println!("{{\"version\":1,\"click_events\":true}}\n[");

    let mut latest: HashMap<usize, OutputChunk> = HashMap::new();
    for w in &wrappers {
        let view = w.view_rx.borrow().clone();
        latest.insert(w.handle, make_chunk(&w.i3_name, cfg.padding, &view));
    }

    let handles: Vec<usize> = wrappers.iter().map(|w| w.handle).collect();
    let handles = i3bar_order(&handles);

    // Periodic output loop. Pure periodic: no reactive flush.
    let mut interval = tokio::time::interval(Duration::from_secs_f64(cfg.min_polling_interval));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Emit an initial line so i3bar has content immediately.
    {
        let mut chunks = Vec::with_capacity(handles.len());
        for h in &handles {
            if let Some(chunk) = latest.get(h) {
                chunks.push(serde_json::to_string(chunk).unwrap_or_default());
            }
        }
        let line = format!("[{}],\n", chunks.join(","));
        let _ = io::stdout().write_all(line.as_bytes());
        let _ = io::stdout().flush();
    }

    loop {
        interval.tick().await;

        for w in &mut wrappers {
            if w.view_rx.has_changed().unwrap_or(false) {
                let _ = w.view_rx.borrow_and_update();
                let view = w.view_rx.borrow().clone();
                latest.insert(w.handle, make_chunk(&w.i3_name, cfg.padding, &view));
            }
        }

        let mut chunks = Vec::with_capacity(handles.len());
        for h in &handles {
            if let Some(chunk) = latest.get(h) {
                chunks.push(serde_json::to_string(chunk).unwrap_or_default());
            }
        }
        let line = format!("[{}],\n", chunks.join(","));
        let _ = io::stdout().write_all(line.as_bytes());
        let _ = io::stdout().flush();
    }
}

pub fn spawn_machine_actor<M: UnitMachine>(
    machine: Arc<M>,
    cfg: crate::config::SchedulingCfg,
    gcfg: GlobalConfig,
    handle: usize,
    click_tx: &broadcast::Sender<crate::core::ClickEvent>,
) -> MachineWrapper {
    let i3_name = format!("{}::{}", machine.name(), handle);
    let i3_name_task = i3_name.clone();
    let (state0, view0, decision0) = machine.init();

    let (view_tx, view_rx) = watch::channel(view0);
    let mut click_rx = click_tx.subscribe();

    tokio::spawn(async move {
        let mut state = state0;

        let poll_timeout = Duration::from_secs(10);
        let poll_backoff =
            Duration::from_secs_f64(cfg.poll_interval.max(gcfg.min_polling_interval));
        let mut next_poll = tokio::time::Instant::now();

        // Always poll immediately if init requested it.
        if decision0 == UnitDecision::PollNow {
            next_poll = tokio::time::Instant::now();
        }

        let poll_interval =
            Duration::from_secs_f64(cfg.poll_interval.max(gcfg.min_polling_interval));
        let tick_interval = Duration::from_secs_f64(gcfg.min_polling_interval);

        let mut poll_tick = tokio::time::interval(poll_interval);
        poll_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        poll_tick.tick().await;

        let mut tick = tokio::time::interval(tick_interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tick.tick().await;

        let mut pending_click: Option<crate::core::ClickEvent> = None;

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let (maybe_view, decision) = machine.on_tick(&mut state);
                    if let Some(view) = maybe_view {
                        let _ = view_tx.send(view);
                    }
                    if decision == UnitDecision::PollNow {
                        next_poll = tokio::time::Instant::now();
                    }
                }
                _ = poll_tick.tick() => {
                    next_poll = tokio::time::Instant::now();
                }
                Ok(click) = click_rx.recv() => {
                    if click.name != i3_name_task {
                        continue;
                    }
                    if pending_click.is_some() {
                        pending_click = Some(click);
                        continue;
                    }

                    let (maybe_view, decision) = machine.on_click(&mut state, click);
                    if let Some(view) = maybe_view {
                        let _ = view_tx.send(view);
                    }
                    if decision == UnitDecision::PollNow {
                        next_poll = tokio::time::Instant::now();
                    }
                }
                () = tokio::time::sleep_until(next_poll) => {
                    // Poll inline. (Clicks cannot interleave in this arm anyway.)
                    let out = match tokio::time::timeout(poll_timeout, machine.poll(&mut state)).await {
                        Ok(Ok(v)) => Ok(v),
                        Ok(Err(e)) => Err(PollError::Unit(e)),
                        Err(_) => Err(PollError::Transport(TransportError::Timeout)),
                    };

                    next_poll = tokio::time::Instant::now() + poll_backoff;

                    let (availability, decision) = match out {
                        Ok(v) => machine.on_poll_ok(&mut state, v),
                        Err(e) => (Availability::Failed(e), UnitDecision::Idle),
                    };
                    let view = render_availability(machine.name(), availability);
                    let _ = view_tx.send(view);
                    if decision == UnitDecision::PollNow {
                        next_poll = tokio::time::Instant::now();
                    }

                    if let Some(click) = pending_click.take() {
                        let (maybe_view, decision) = machine.on_click(&mut state, click);
                        if let Some(view) = maybe_view {
                            let _ = view_tx.send(view);
                        }
                        if decision == UnitDecision::PollNow {
                            next_poll = tokio::time::Instant::now();
                        }
                    }
                }
            }
        }
    });

    MachineWrapper {
        i3_name,
        handle,
        view_rx,
    }
}
