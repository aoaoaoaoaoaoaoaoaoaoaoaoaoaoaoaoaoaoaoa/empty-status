use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Ema {
    tau: Duration,
    value: Option<(f64, Instant)>,
}

impl Ema {
    pub const fn new(tau: Duration) -> Self {
        Self { tau, value: None }
    }

    pub fn reset(&mut self) {
        self.value = None;
    }

    pub fn push(&mut self, sample: f64, at: Instant) -> f64 {
        let value = self.value.map_or(sample, |(previous, previous_at)| {
            let weight = (-at.saturating_duration_since(previous_at).as_secs_f64()
                / self.tau.as_secs_f64())
            .exp();
            previous * weight + sample * (1.0 - weight)
        });
        self.value = Some((value, at));
        value
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::Ema;

    #[test]
    fn smooths_a_step() {
        let mut ema = Ema::new(Duration::from_secs(1));
        let start = Instant::now();
        assert_eq!(ema.push(0.0, start), 0.0);
        let value = ema.push(10.0, start + Duration::from_secs(1));
        assert!((value - 10.0 * (1.0 - (-1.0_f64).exp())).abs() < 1e-9);
    }
}
