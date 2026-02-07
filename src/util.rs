use std::{
    marker::PhantomData,
    ops::{Add, Mul},
    time::Instant,
};

pub trait RotateEnum: Sized + Copy {
    fn next(self) -> Self;
}

#[macro_export]
macro_rules! impl_handle_click_nop {
    () => {
        fn handle_click(&mut self, _click: $crate::core::ClickEvent) {}
    };
}

#[macro_export]
macro_rules! impl_handle_click_rotate_mode {
    () => {
        fn handle_click(&mut self, _click: $crate::core::ClickEvent) {
            // self.mode = self.mode.next();
            self.mode = $crate::util::RotateEnum::next(self.mode);
        }
    };
}

#[macro_export]
macro_rules! mode_enum {
    ( $($member:ident),* $(,)? ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, empty_status_macros::RotateNext)]
        pub enum DisplayMode {
            $($member),*
        }

        impl $crate::util::RotateEnum for DisplayMode {
            fn next(self) -> Self {
                DisplayMode::next(self)
            }
        }
    };
}

pub trait Smoother<T>
where
    T: Add<T, Output = T> + Mul<f64, Output = T>,
{
    fn feed(&mut self, value: T, time: Instant);
    fn read(&self) -> Option<&T>;

    fn feed_and_read(&mut self, value: T, time: Instant) -> Option<&T> {
        self.feed(value, time);
        self.read()
    }
}

#[derive(Debug)]
struct EmaRecord<T>
where
    T: Add<T, Output = T> + Mul<f64, Output = T>,
{
    v: T,
    t: Instant,
}

#[derive(Debug)]
pub struct Ema<T>
where
    T: Add<T, Output = T> + Mul<f64, Output = T>,
{
    lambda_sec: f64,
    value: Option<EmaRecord<T>>,
    _marker: std::marker::PhantomData<T>,
}

impl<T> Ema<T>
where
    T: Add<T, Output = T> + Mul<f64, Output = T>,
{
    pub fn new(lambda_sec: f64) -> Self {
        Ema {
            lambda_sec,
            value: None,
            _marker: PhantomData,
        }
    }
}

impl<T: Into<f64>> Smoother<T> for Ema<T>
where
    T: Add<T, Output = T> + Mul<f64, Output = T>,
{
    fn feed(&mut self, value: T, time: Instant) {
        match self.value.take() {
            None => {
                self.value = Some(EmaRecord { v: value, t: time });
            }
            Some(EmaRecord { v, t }) => {
                let dt = time.duration_since(t);
                let f = (-dt.as_secs_f64() / self.lambda_sec).exp();
                let new_v = v * f + value * (1.0 - f);
                self.value = Some(EmaRecord { v: new_v, t: time });
            }
        }
    }
    fn read(&self) -> Option<&T> {
        self.value.as_ref().map(|r| &r.v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64;
    use std::time::Duration;

    #[test]
    fn ema_basic() {
        // λ = 1 s
        let mut s = Ema::new(1.0);
        let t0 = Instant::now();

        // Step 0: initialise with 0
        s.feed(0.0, t0);
        assert!((s.read().unwrap() - 0.0).abs() < 1e-12);

        // Step 1: after 1 s feed 10
        s.feed(10.0, t0 + Duration::from_secs(1));
        let v1 = *s.read().unwrap();
        let expected_v1 = 10.0 * (1.0 - f64::consts::E.powf(-1.0));
        assert!((v1 - expected_v1).abs() < 1e-9);

        // Step 2: after another 1 s feed another 10
        s.feed(10.0, t0 + Duration::from_secs(2));
        let v2 = s.read().unwrap();
        let expected_v2 = v1 * f64::consts::E.powf(-1.0) + 10.0 * (1.0 - f64::consts::E.powf(-1.0));
        assert!((v2 - expected_v2).abs() < 1e-9);
    }

    #[test]
    fn ema_empty() {
        let s: Ema<f64> = Ema::new(1.0);
        assert!(s.read().is_none());
    }
}
