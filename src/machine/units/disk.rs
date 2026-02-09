use crate::machine::effects::{DirEntries, EffectReq, FsListDir, FsRead};
use crate::machine::types::{Availability, Health, UnitDecision, UnitMachine, View};
use crate::render::markup::Markup;
use crate::units::disk::{Disk, DiskConfig};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct DiskMachine {
    cfg: DiskConfig,
}

impl DiskMachine {
    pub fn new(cfg: DiskConfig) -> Self {
        Self { cfg }
    }
}

#[derive(Debug)]
pub struct State {
    unit: Disk,
}

#[derive(Debug, Clone)]
pub struct UnitErr(String);

impl std::fmt::Display for UnitErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UnitErr {}

impl UnitMachine for DiskMachine {
    type PollOut = Markup;
    type State = State;
    type UnitError = UnitErr;

    fn name(&self) -> &'static str {
        "Disk"
    }

    fn init(&self) -> (Self::State, View, UnitDecision) {
        let unit = Disk::from_cfg(self.cfg.clone());
        Disk::fix_up_and_validate();
        let view = View {
            body: Markup::text("disk ") + Markup::text("loading").fg(crate::core::VIOLET),
            health: Health::Degraded,
        };

        (State { unit }, view, UnitDecision::PollNow)
    }

    fn on_tick(&self, _state: &mut Self::State) -> (Option<View>, UnitDecision) {
        (None, UnitDecision::Idle)
    }

    fn on_click(
        &self,
        _state: &mut Self::State,
        click: crate::core::ClickEvent,
    ) -> (Option<View>, UnitDecision) {
        Disk::handle_click(click);
        (None, UnitDecision::PollNow)
    }

    async fn poll(
        &self,
        effects: &crate::machine::effects::EffectEngine,
        state: &mut Self::State,
    ) -> Result<Self::PollOut, crate::machine::types::PollError<Self::UnitError>> {
        if state.unit.disk_name().is_none() {
            if let Some(name) = resolve_disk_name(effects, &state.unit).await? {
                state.unit.set_disk_name(name);
            }
        }

        let list_out = effects
            .run(EffectReq::FsListDir(FsListDir {
                key: crate::machine::effects::DirKey::new("sys/block"),
                path: "/sys/block".into(),
                cache_fresh_for: Duration::from_secs(60),
            }))
            .await?;
        let entries = list_out.expect::<DirEntries>()?.0;
        state.unit.select_root(&entries);

        let mut sector_size_bytes = None;
        if let Some(root) = state.unit.disk_root() {
            let out = effects
                .run(EffectReq::FsRead(FsRead {
                    key: crate::machine::effects::FsKey::new(format!(
                        "sys/block/{root}/queue/hw_sector_size"
                    )),
                    path: format!("/sys/block/{root}/queue/hw_sector_size").into(),
                    cache_fresh_for: Duration::from_secs(3600),
                }))
                .await?;
            sector_size_bytes = Some(out.expect::<bytes::Bytes>()?);
        }

        let Some(disk_name) = state.unit.disk_name() else {
            return Ok(state
                .unit
                .read_markup_from_bytes(&[], sector_size_bytes.as_deref()));
        };

        let stat = effects
            .run(EffectReq::FsRead(FsRead {
                key: crate::machine::effects::FsKey::new(format!(
                    "sys/class/block/{disk_name}/stat"
                )),
                path: format!("/sys/class/block/{disk_name}/stat").into(),
                cache_fresh_for: Duration::from_millis(150),
            }))
            .await?;
        let stat_bytes = stat.expect::<bytes::Bytes>()?;
        Ok(state
            .unit
            .read_markup_from_bytes(&stat_bytes, sector_size_bytes.as_deref()))
    }

    fn on_poll_ok(
        &self,
        _state: &mut Self::State,
        body: Self::PollOut,
    ) -> (
        Availability<Markup, crate::machine::types::PollError<Self::UnitError>>,
        UnitDecision,
    ) {
        (Availability::Ready(body), UnitDecision::Idle)
    }
}

async fn resolve_disk_name(
    _effects: &crate::machine::effects::EffectEngine,
    unit: &Disk,
) -> Result<Option<String>, crate::machine::types::PollError<UnitErr>> {
    if let Some(label) = unit.selector_partlabel() {
        let label_path = format!("/dev/disk/by-partlabel/{label}");
        if let Ok(path) = tokio::fs::read_link(&label_path).await {
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                return Ok(Some(name.to_string()));
            }
        }
    }
    if let Some(uuid) = unit.selector_partuuid() {
        let uuid_path = format!("/dev/disk/by-partuuid/{uuid}");
        if let Ok(path) = tokio::fs::read_link(&uuid_path).await {
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                return Ok(Some(name.to_string()));
            }
        }
    }
    Ok(None)
}
