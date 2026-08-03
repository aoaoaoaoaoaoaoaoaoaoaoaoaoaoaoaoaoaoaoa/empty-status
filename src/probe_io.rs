use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use reqwest::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    Timeout,
    Http(u16),
    Message(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => formatter.write_str("timeout"),
            Self::Http(status) => write!(formatter, "HTTP {status}"),
            Self::Message(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<std::io::Error> for TransportError {
    fn from(error: std::io::Error) -> Self {
        Self::Message(error.to_string())
    }
}

#[derive(Debug)]
pub struct ProbeIo {
    client: reqwest::Client,
}

impl ProbeIo {
    pub fn new() -> Result<Arc<Self>, TransportError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("empty-status/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| TransportError::Message(error.to_string()))?;
        Ok(Arc::new(Self { client }))
    }

    pub async fn read(&self, path: impl AsRef<Path>) -> Result<Vec<u8>, TransportError> {
        tokio::fs::read(path).await.map_err(Into::into)
    }

    pub async fn read_link(&self, path: impl AsRef<Path>) -> Result<PathBuf, TransportError> {
        tokio::fs::read_link(path).await.map_err(Into::into)
    }

    pub async fn get(&self, url: Url) -> Result<Vec<u8>, TransportError> {
        tracing::debug!(%url, "HTTP GET");
        self.execute(self.client.get(url)).await
    }

    pub async fn get_bearer(&self, url: Url, token: &str) -> Result<Vec<u8>, TransportError> {
        tracing::debug!(%url, "authenticated HTTP GET");
        self.execute(self.client.get(url).bearer_auth(token)).await
    }

    async fn execute(&self, request: reqwest::RequestBuilder) -> Result<Vec<u8>, TransportError> {
        let response = request
            .send()
            .await
            .map_err(|error| TransportError::Message(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(TransportError::Http(status.as_u16()));
        }
        response
            .bytes()
            .await
            .map(|body| body.to_vec())
            .map_err(|error| TransportError::Message(error.to_string()))
    }

    pub async fn blocking<F, T>(&self, operation: F) -> Result<T, TransportError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        tokio::task::spawn_blocking(operation)
            .await
            .map_err(|error| TransportError::Message(format!("blocking probe failed: {error}")))
    }
}
