use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use route_ratelimit::{RateLimitMiddleware, ThrottleBehavior};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct RateLimitSpec {
    pub per: Duration,
    pub burst: u32,
}

pub fn make_http_client(host: &str, spec: RateLimitSpec) -> anyhow::Result<ClientWithMiddleware> {
    let client = reqwest::Client::builder().build()?;

    let mw = RateLimitMiddleware::builder()
        .host(host, |h| {
            h.route(|r| {
                r.limit(spec.burst, spec.per)
                    .on_limit(ThrottleBehavior::Delay)
            })
        })
        .build();

    Ok(ClientBuilder::new(client).with(mw).build())
}

#[derive(Debug, Default)]
pub struct ClientPool {
    inner: Mutex<HashMap<String, ClientWithMiddleware>>,
}

impl ClientPool {
    pub fn client_for_host(
        &self,
        host: &str,
        spec: RateLimitSpec,
    ) -> anyhow::Result<ClientWithMiddleware> {
        let mut g = self.inner.lock().unwrap();
        if let Some(c) = g.get(host) {
            return Ok(c.clone());
        }

        let c = make_http_client(host, spec)?;
        g.insert(host.to_string(), c.clone());
        Ok(c)
    }
}
