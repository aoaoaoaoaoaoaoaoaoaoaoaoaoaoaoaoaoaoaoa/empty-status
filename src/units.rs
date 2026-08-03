use std::{fmt, future::Future, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{
    core::{Button, View},
    probe_io::{ProbeIo, TransportError},
};

macro_rules! cycle {
    ($visibility:vis enum $name:ident { $first:ident, $($rest:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $visibility enum $name {
            $first,
            $($rest),+
        }

        impl $name {
            fn advance(&mut self) {
                *self = cycle!(@match *self; $first; []; $first, $($rest),+);
            }
        }

        impl $crate::units::Cycle for $name {
            fn advance(&mut self) {
                $name::advance(self);
            }
        }

        impl $crate::units::RestorableCycle for $name {
            fn point(&self) -> &'static str {
                match self {
                    Self::$first => stringify!($first),
                    $(Self::$rest => stringify!($rest)),+
                }
            }

            fn seek(&mut self, point: &str) -> bool {
                match point {
                    stringify!($first) => *self = Self::$first,
                    $(stringify!($rest) => *self = Self::$rest,)+
                    _ => return false,
                }
                true
            }
        }
    };
    (@match $value:expr; $first:ident; [$($arms:tt)*]; $point:ident, $next:ident, $($rest:ident),+) => {
        cycle!(@match $value; $first; [$($arms)* Self::$point => Self::$next,]; $next, $($rest),+)
    };
    (@match $value:expr; $first:ident; [$($arms:tt)*]; $point:ident, $last:ident) => {
        match $value {
            $($arms)*
            Self::$point => Self::$last,
            Self::$last => Self::$first,
        }
    };
}

macro_rules! persist {
    ($model:ident.$field:ident) => {
        impl $crate::units::Persistent for $model {
            fn posture(&self) -> $crate::units::Posture {
                $crate::units::Posture::from_cycle(&self.$field)
            }

            fn restore(&mut self, posture: &$crate::units::Posture) -> bool {
                posture.restore_cycle(&mut self.$field)
            }
        }
    };
    ($model:ident.$field:ident, orbit) => {
        impl $crate::units::Persistent for $model {
            fn posture(&self) -> $crate::units::Posture {
                $crate::units::Posture::from_orbit(&self.$field)
            }

            fn restore(&mut self, posture: &$crate::units::Posture) -> bool {
                posture.restore_orbit(&mut self.$field)
            }
        }
    };
}

pub mod bat;
pub mod cpu;
pub mod disk;
pub mod mem;
pub mod net;
pub mod quota;
pub mod time;
pub mod weather;
pub mod wifi;

pub trait Cycle {
    fn advance(&mut self);
}

pub trait RestorableCycle: Cycle + Clone {
    fn point(&self) -> &'static str;
    fn seek(&mut self, point: &str) -> bool;
}

trait Persistent {
    fn posture(&self) -> Posture;
    fn restore(&mut self, posture: &Posture) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Posture(Box<[String]>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiniteCycle<T> {
    points: Box<[T]>,
    focus: usize,
}

impl<T> FiniteCycle<T> {
    pub fn new(first: T, rest: impl IntoIterator<Item = T>) -> Self {
        let mut points = vec![first];
        points.extend(rest);
        Self {
            points: points.into_boxed_slice(),
            focus: 0,
        }
    }

    pub fn focus(&self) -> &T {
        &self.points[self.focus]
    }

    pub fn points(&self) -> &[T] {
        &self.points
    }

    pub fn seek(&mut self, predicate: impl Fn(&T) -> bool) -> bool {
        let Some(focus) = self.points.iter().position(predicate) else {
            return false;
        };
        self.focus = focus;
        true
    }
}

impl<T> Cycle for FiniteCycle<T> {
    fn advance(&mut self) {
        self.focus = (self.focus + 1) % self.points.len();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseOrbit<L, R> {
    left: L,
    right: R,
}

impl<L, R> MouseOrbit<L, R> {
    pub const fn new(left: L, right: R) -> Self {
        Self { left, right }
    }

    pub const fn left(&self) -> &L {
        &self.left
    }

    pub const fn right(&self) -> &R {
        &self.right
    }
}

impl<L: Cycle, R: Cycle> MouseOrbit<L, R> {
    pub fn act(&mut self, button: Button) -> bool {
        match button {
            Button::Left => self.left.advance(),
            Button::Right => self.right.advance(),
            Button::Middle | Button::Other(_) => return false,
        }
        true
    }
}

impl Posture {
    fn from_cycle<C: RestorableCycle>(cycle: &C) -> Self {
        Self(vec![cycle.point().to_owned()].into_boxed_slice())
    }

    fn from_orbit<L: RestorableCycle, R: RestorableCycle>(orbit: &MouseOrbit<L, R>) -> Self {
        Self(
            vec![
                orbit.left.point().to_owned(),
                orbit.right.point().to_owned(),
            ]
            .into_boxed_slice(),
        )
    }

    fn restore_cycle<C: RestorableCycle>(&self, cycle: &mut C) -> bool {
        let [point] = self.0.as_ref() else {
            return false;
        };
        cycle.seek(point)
    }

    fn restore_orbit<L: RestorableCycle, R: RestorableCycle>(
        &self,
        orbit: &mut MouseOrbit<L, R>,
    ) -> bool {
        let [left, right] = self.0.as_ref() else {
            return false;
        };
        let original = orbit.clone();
        if orbit.left.seek(left) && orbit.right.seek(right) {
            true
        } else {
            *orbit = original;
            false
        }
    }
}

#[derive(Debug)]
pub enum ProbeError {
    Transport(TransportError),
    Unit(String),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => error.fmt(f),
            Self::Unit(message) => f.write_str(message),
        }
    }
}

impl From<TransportError> for ProbeError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

#[derive(Debug)]
pub enum Reaction {
    Inert,
    Refresh,
    Publish(View),
}

impl Reaction {
    pub const fn inert() -> Self {
        Self::Inert
    }

    pub const fn refresh() -> Self {
        Self::Refresh
    }

    pub const fn publish(view: View) -> Self {
        Self::Publish(view)
    }
}

#[derive(Debug)]
pub enum Unit {
    Bat(bat::Model),
    Cpu(cpu::Model),
    Disk(disk::Model),
    Mem(mem::Model),
    Net(net::Model),
    Quota(quota::Model),
    Time(time::Model),
    Weather(weather::Model),
    Wifi(wifi::Model),
}

#[derive(Debug)]
pub enum Request {
    Bat(bat::Request),
    Cpu(cpu::Request),
    Disk(disk::Request),
    Mem(mem::Request),
    Net(net::Request),
    Quota(quota::Request),
    Time(time::Request),
    Weather(weather::Request),
    Wifi(wifi::Request),
}

#[derive(Debug)]
pub enum Reply {
    Bat(bat::Reply),
    Cpu(cpu::Reply),
    Disk(disk::Reply),
    Mem(mem::Reply),
    Net(net::Reply),
    Quota(quota::Reply),
    Time(time::Reply),
    Weather(weather::Reply),
    Wifi(wifi::Reply),
}

impl Unit {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Bat(_) => "Bat",
            Self::Cpu(_) => "Cpu",
            Self::Disk(_) => "Disk",
            Self::Mem(_) => "Mem",
            Self::Net(_) => "Net",
            Self::Quota(_) => "Quota",
            Self::Time(_) => "Time",
            Self::Weather(_) => "Weather",
            Self::Wifi(_) => "Wifi",
        }
    }

    pub fn initial_view(&self) -> View {
        View::loading(&self.name().to_ascii_lowercase())
    }

    pub fn canonical_cadence(&self, requested: Duration) -> Duration {
        let floor = match self {
            Self::Quota(_) => Duration::from_secs(15),
            Self::Weather(_) => Duration::from_mins(2),
            Self::Bat(_)
            | Self::Cpu(_)
            | Self::Disk(_)
            | Self::Mem(_)
            | Self::Net(_)
            | Self::Time(_)
            | Self::Wifi(_) => Duration::ZERO,
        };
        requested.max(floor)
    }

    pub fn request(&self) -> Request {
        match self {
            Self::Bat(model) => Request::Bat(model.request()),
            Self::Cpu(_) => Request::Cpu(cpu::Model::request()),
            Self::Disk(model) => Request::Disk(model.request()),
            Self::Mem(model) => Request::Mem(model.request()),
            Self::Net(model) => Request::Net(model.request()),
            Self::Quota(model) => Request::Quota(model.request()),
            Self::Time(model) => Request::Time(model.request()),
            Self::Weather(model) => Request::Weather(model.request()),
            Self::Wifi(model) => Request::Wifi(model.request()),
        }
    }

    pub fn apply(&mut self, reply: Reply) -> Result<View, &'static str> {
        match (self, reply) {
            (Self::Bat(model), Reply::Bat(reply)) => Ok(model.apply(reply)),
            (Self::Cpu(model), Reply::Cpu(reply)) => Ok(model.apply(reply)),
            (Self::Disk(model), Reply::Disk(reply)) => Ok(model.apply(reply)),
            (Self::Mem(_), Reply::Mem(reply)) => Ok(mem::Model::apply(reply)),
            (Self::Net(model), Reply::Net(reply)) => Ok(model.apply(reply)),
            (Self::Quota(model), Reply::Quota(reply)) => Ok(model.apply(reply)),
            (Self::Time(model), Reply::Time(reply)) => Ok(model.apply(reply)),
            (Self::Weather(model), Reply::Weather(reply)) => Ok(model.apply(reply)),
            (Self::Wifi(model), Reply::Wifi(reply)) => Ok(model.apply(reply)),
            _ => Err("unit/request registry invariant violated"),
        }
    }

    pub fn click(&mut self, button: Button) -> Reaction {
        match self {
            Self::Bat(model) => model.click(button),
            Self::Cpu(model) => model.click(button),
            Self::Disk(_) => disk::Model::click(button),
            Self::Mem(model) => model.click(button),
            Self::Net(model) => model.click(button),
            Self::Quota(model) => model.click(button),
            Self::Time(model) => model.click(button),
            Self::Weather(model) => model.click(button),
            Self::Wifi(model) => model.click(button),
        }
    }

    pub fn posture(&self) -> Option<Posture> {
        match self {
            Self::Bat(model) => Some(model.posture()),
            Self::Cpu(model) => Some(model.posture()),
            Self::Disk(_) => None,
            Self::Mem(model) => Some(model.posture()),
            Self::Net(model) => Some(model.posture()),
            Self::Quota(model) => Some(model.posture()),
            Self::Time(model) => Some(model.posture()),
            Self::Weather(model) => Some(model.posture()),
            Self::Wifi(model) => Some(model.posture()),
        }
    }

    pub fn restore(&mut self, posture: &Posture) -> bool {
        match self {
            Self::Bat(model) => model.restore(posture),
            Self::Cpu(model) => model.restore(posture),
            Self::Disk(_) => false,
            Self::Mem(model) => model.restore(posture),
            Self::Net(model) => model.restore(posture),
            Self::Quota(model) => model.restore(posture),
            Self::Time(model) => model.restore(posture),
            Self::Weather(model) => model.restore(posture),
            Self::Wifi(model) => model.restore(posture),
        }
    }
}

async fn within<T>(
    limit: Duration,
    probe: impl Future<Output = Result<T, ProbeError>>,
) -> Result<T, ProbeError> {
    tokio::time::timeout(limit, probe)
        .await
        .map_err(|_| ProbeError::Transport(TransportError::Timeout))?
}

impl Request {
    pub async fn execute(self, io: Arc<ProbeIo>) -> Reply {
        match self {
            Self::Bat(request) => Reply::Bat(within(bat::TIMEOUT, bat::probe(request, &io)).await),
            Self::Cpu(request) => Reply::Cpu(within(cpu::TIMEOUT, cpu::probe(request, &io)).await),
            Self::Disk(request) => {
                Reply::Disk(within(disk::TIMEOUT, disk::probe(request, &io)).await)
            }
            Self::Mem(request) => Reply::Mem(within(mem::TIMEOUT, mem::probe(request, &io)).await),
            Self::Net(request) => Reply::Net(within(net::TIMEOUT, net::probe(request, &io)).await),
            Self::Quota(request) => {
                Reply::Quota(within(quota::TIMEOUT, quota::probe(request, &io)).await)
            }
            Self::Time(request) => {
                Reply::Time(within(time::TIMEOUT, time::probe(request, &io)).await)
            }
            Self::Weather(request) => {
                Reply::Weather(within(weather::TIMEOUT, weather::probe(request, &io)).await)
            }
            Self::Wifi(request) => {
                Reply::Wifi(within(wifi::TIMEOUT, wifi::probe(request, &io)).await)
            }
        }
    }
}

pub fn error_view(label: &str, error: ProbeError) -> View {
    View::error(label, error.to_string())
}

pub fn positive_duration(name: &str, seconds: f64) -> Result<Duration, String> {
    let duration = Duration::try_from_secs_f64(seconds)
        .map_err(|error| format!("{name} is not a valid duration: {error}"))?;
    if duration.is_zero() {
        Err(format!("{name} must be positive"))
    } else if duration > Duration::from_hours(8_766) {
        Err(format!("{name} cannot exceed one year"))
    } else {
        Ok(duration)
    }
}

#[cfg(test)]
mod tests {
    use super::{Button, Cycle, FiniteCycle, MouseOrbit, Posture};

    cycle!(
        enum Bit {
            Zero,
            One,
        }
    );

    cycle!(
        enum Trit {
            Zero,
            One,
            Two,
        }
    );

    #[test]
    fn mouse_orbit_obeys_klein_group_laws_for_binary_axes() {
        let identity = MouseOrbit::new(Bit::Zero, Bit::Zero);

        let mut left_squared = identity;
        assert!(left_squared.act(Button::Left));
        assert!(left_squared.act(Button::Left));
        assert_eq!(left_squared, identity);

        let mut right_squared = identity;
        assert!(right_squared.act(Button::Right));
        assert!(right_squared.act(Button::Right));
        assert_eq!(right_squared, identity);

        let mut left_right = identity;
        assert!(left_right.act(Button::Left));
        assert!(left_right.act(Button::Right));
        let mut right_left = identity;
        assert!(right_left.act(Button::Right));
        assert!(right_left.act(Button::Left));
        assert_eq!(left_right, right_left);
        assert_ne!(left_right, identity);

        assert!(!left_right.act(Button::Middle));
        assert!(!left_right.act(Button::Other(8)));

        let mut nonbinary = MouseOrbit::new(Trit::Zero, Bit::Zero);
        assert!(nonbinary.act(Button::Left));
        assert!(nonbinary.act(Button::Left));
        assert!(nonbinary.act(Button::Left));
        assert_eq!(nonbinary, MouseOrbit::new(Trit::Zero, Bit::Zero));
    }

    #[test]
    fn finite_cycle_is_nonempty_and_wraps() {
        let mut cycle = FiniteCycle::new("a", ["b", "c"]);
        assert_eq!(cycle.points(), ["a", "b", "c"]);
        for expected in ["a", "b", "c", "a"] {
            assert_eq!(*cycle.focus(), expected);
            cycle.advance();
        }
    }

    #[test]
    fn named_posture_restores_an_orbit_atomically() {
        let mut source = MouseOrbit::new(Bit::Zero, Trit::Zero);
        assert!(source.act(Button::Left));
        assert!(source.act(Button::Right));
        assert!(source.act(Button::Right));
        let encoded = serde_json::to_string(&Posture::from_orbit(&source));
        assert!(encoded.is_ok());
        let Ok(encoded) = encoded else { return };
        let posture = serde_json::from_str::<Posture>(&encoded);
        assert!(posture.is_ok());
        let Ok(posture) = posture else { return };
        let mut restored = MouseOrbit::new(Bit::Zero, Trit::Zero);
        assert!(posture.restore_orbit(&mut restored));
        assert_eq!(restored, source);

        let invalid = serde_json::from_str::<Posture>(r#"["One","Vanished"]"#);
        assert!(invalid.is_ok());
        let Ok(invalid) = invalid else { return };
        let original = restored;
        assert!(!invalid.restore_orbit(&mut restored));
        assert_eq!(restored, original);
    }
}
