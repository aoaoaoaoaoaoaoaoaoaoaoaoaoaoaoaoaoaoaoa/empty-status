use crate::machine::types::TransportError;
use reqwest::Url;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncBufReadExt;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HttpCacheKey(String);

impl HttpCacheKey {
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone)]
pub enum EffectReq {
    HttpGet(HttpGet),
    ProcBatch(ProcBatch),
    ProcExec(ProcExec),
    FsRead(FsRead),
    FsListDir(FsListDir),
}

#[derive(Debug, Clone)]
pub struct HttpGet {
    pub key: HttpCacheKey,
    pub url: Url,
    pub policy: HttpPolicy,
}

#[derive(Debug, Clone, Copy)]
pub struct HttpPolicy {
    pub rate: crate::machine::http::RateLimitSpec,
    pub cache_fresh_for: Duration,
}

#[derive(Debug, Clone)]
pub enum EffectOut {
    Http(HttpResponse),
    ProcLines(Vec<String>),
    FsBytes(bytes::Bytes),
    DirEntries(DirEntries),
}

pub trait EffectOutExpect: Sized {
    fn expect_from(out: EffectOut) -> anyhow::Result<Self>;
}

impl EffectOut {
    pub fn expect<T: EffectOutExpect>(self) -> anyhow::Result<T> {
        T::expect_from(self)
    }
}

impl EffectOutExpect for HttpResponse {
    fn expect_from(out: EffectOut) -> anyhow::Result<Self> {
        match out {
            EffectOut::Http(v) => Ok(v),
            EffectOut::ProcLines(_) | EffectOut::FsBytes(_) | EffectOut::DirEntries(_) => {
                anyhow::bail!("unexpected effect output")
            }
        }
    }
}

impl EffectOutExpect for Vec<String> {
    fn expect_from(out: EffectOut) -> anyhow::Result<Self> {
        match out {
            EffectOut::ProcLines(v) => Ok(v),
            EffectOut::Http(_) | EffectOut::FsBytes(_) | EffectOut::DirEntries(_) => {
                anyhow::bail!("unexpected effect output")
            }
        }
    }
}

impl EffectOutExpect for bytes::Bytes {
    fn expect_from(out: EffectOut) -> anyhow::Result<Self> {
        match out {
            EffectOut::FsBytes(v) => Ok(v),
            EffectOut::Http(_) | EffectOut::ProcLines(_) | EffectOut::DirEntries(_) => {
                anyhow::bail!("unexpected effect output")
            }
        }
    }
}

impl EffectOutExpect for DirEntries {
    fn expect_from(out: EffectOut) -> anyhow::Result<Self> {
        match out {
            EffectOut::DirEntries(v) => Ok(v),
            EffectOut::Http(_) | EffectOut::ProcLines(_) | EffectOut::FsBytes(_) => {
                anyhow::bail!("unexpected effect output")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FsKey(String);

impl FsKey {
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone)]
pub struct FsRead {
    pub key: FsKey,
    pub path: std::path::PathBuf,
    pub cache_fresh_for: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DirKey(String);

impl DirKey {
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone)]
pub struct FsListDir {
    pub key: DirKey,
    pub path: std::path::PathBuf,
    pub cache_fresh_for: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProcKey(String);

impl ProcKey {
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone)]
pub struct ProcBatch {
    pub key: ProcKey,
    pub cmd: Vec<String>,
    pub max_lines: usize,
    pub startup_grace: Duration,
}

#[derive(Debug, Clone)]
pub struct ProcExec {
    pub cmd: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DirEntries(pub Vec<String>);

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub body: bytes::Bytes,
}

#[derive(Debug)]
struct HttpCacheEntry {
    fresh_until: Instant,
    response: HttpResponse,
}

#[derive(Debug, Default)]
struct HttpState {
    cache: HashMap<HttpCacheKey, HttpCacheEntry>,
}

#[derive(Debug, Default)]
pub struct EffectEngine {
    http: Mutex<HttpState>,
    clients: crate::machine::http::ClientPool,
    procs: Mutex<HashMap<ProcKey, ProcState>>,
    fs: Mutex<HashMap<FsKey, FsCacheEntry>>,
    dirs: Mutex<HashMap<DirKey, DirCacheEntry>>,
    http_log: Option<std::sync::Mutex<std::fs::File>>,
}

#[derive(Debug, Clone)]
struct FsCacheEntry {
    fresh_until: Instant,
    bytes: bytes::Bytes,
}

#[derive(Debug, Clone)]
struct DirCacheEntry {
    fresh_until: Instant,
    entries: DirEntries,
}

#[derive(Debug)]
struct ProcState {
    child: tokio::process::Child,
    rx: tokio::sync::mpsc::UnboundedReceiver<String>,
}

impl EffectEngine {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            http: Mutex::default(),
            clients: crate::machine::http::ClientPool::default(),
            procs: Mutex::default(),
            fs: Mutex::default(),
            dirs: Mutex::default(),
            http_log: Self::open_http_log(),
        })
    }

    fn open_http_log() -> Option<std::sync::Mutex<std::fs::File>> {
        let bd = xdg::BaseDirectories::with_prefix("empty-status");
        let log_dir = bd.get_state_home()?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join("http.log"))
            .ok()?;
        Some(std::sync::Mutex::new(file))
    }

    fn log_http(&self, line: &str) {
        let Some(log) = self.http_log.as_ref() else {
            return;
        };
        if let Ok(mut file) = log.lock() {
            let ts = chrono::Utc::now().to_rfc3339();
            let _ = writeln!(file, "{ts} {line}");
        }
    }

    pub async fn run(&self, req: EffectReq) -> Result<EffectOut, TransportError> {
        match req {
            EffectReq::HttpGet(get) => self.http_get(get).await.map(EffectOut::Http),
            EffectReq::ProcBatch(pb) => self.proc_batch(pb).await.map(EffectOut::ProcLines),
            EffectReq::ProcExec(px) => self.proc_exec(px).await.map(EffectOut::ProcLines),
            EffectReq::FsRead(fr) => self.fs_read(fr).await.map(EffectOut::FsBytes),
            EffectReq::FsListDir(fr) => self.fs_list_dir(fr).await.map(EffectOut::DirEntries),
        }
    }

    async fn fs_read(&self, fr: FsRead) -> Result<bytes::Bytes, TransportError> {
        let now = Instant::now();
        {
            let cache = self.fs.lock().await;
            if let Some(ent) = cache.get(&fr.key)
                && now < ent.fresh_until
            {
                return Ok(ent.bytes.clone());
            }
        }

        let bytes = tokio::fs::read(fr.path)
            .await
            .map(bytes::Bytes::from)
            .map_err(|e| TransportError::Transport(e.to_string()))?;

        let mut cache = self.fs.lock().await;
        let _ = cache.insert(
            fr.key,
            FsCacheEntry {
                fresh_until: now + fr.cache_fresh_for,
                bytes: bytes.clone(),
            },
        );

        Ok(bytes)
    }

    async fn fs_list_dir(&self, fr: FsListDir) -> Result<DirEntries, TransportError> {
        let now = Instant::now();
        {
            let cache = self.dirs.lock().await;
            if let Some(ent) = cache.get(&fr.key)
                && now < ent.fresh_until
            {
                return Ok(ent.entries.clone());
            }
        }

        let mut out = Vec::new();
        let mut dir = tokio::fs::read_dir(&fr.path)
            .await
            .map_err(|e| TransportError::Transport(e.to_string()))?;
        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| TransportError::Transport(e.to_string()))?
        {
            out.push(entry.file_name().to_string_lossy().to_string());
        }
        let entries = DirEntries(out);

        let mut cache = self.dirs.lock().await;
        let _ = cache.insert(
            fr.key,
            DirCacheEntry {
                fresh_until: now + fr.cache_fresh_for,
                entries: entries.clone(),
            },
        );
        Ok(entries)
    }

    async fn proc_batch(&self, pb: ProcBatch) -> Result<Vec<String>, TransportError> {
        let mut procs = self.procs.lock().await;
        let mut spawned_now = false;
        if !procs.contains_key(&pb.key) {
            let mut it = pb.cmd.iter();
            let exe = it
                .next()
                .ok_or_else(|| TransportError::Transport("empty command".into()))?;
            let mut cmd = tokio::process::Command::new(exe);
            for arg in it {
                let _ = cmd.arg(arg);
            }
            let _ = cmd
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped());

            let mut child = cmd
                .spawn()
                .map_err(|e| TransportError::Transport(e.to_string()))?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| TransportError::Transport("missing stdout".into()))?;

            let reader = tokio::io::BufReader::new(stdout);
            let mut lines = reader.lines();
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

            drop(tokio::spawn(async move {
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = tx.send(line);
                }
            }));

            let _ = procs.insert(pb.key.clone(), ProcState { child, rx });
            spawned_now = true;
        }

        let mut drop_proc = None;
        let st = procs
            .get_mut(&pb.key)
            .ok_or_else(|| TransportError::Transport("proc missing".into()))?;

        let mut out = Vec::new();
        let mut channel_closed = false;
        for _ in 0..pb.max_lines {
            match st.rx.try_recv() {
                Ok(line) => out.push(line),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    channel_closed = true;
                    break;
                }
            }
        }

        if out.is_empty() && spawned_now && drop_proc.is_none() {
            match tokio::time::timeout(pb.startup_grace, st.rx.recv()).await {
                Ok(Some(line)) => out.push(line),
                Ok(None) => {
                    channel_closed = true;
                }
                Err(_) => {}
            }
        }

        if out.is_empty() && drop_proc.is_none() {
            match st.child.try_wait() {
                Ok(Some(status)) => {
                    drop_proc = Some(format!("proc exited: {status}"));
                }
                Ok(None) => {}
                Err(error) => {
                    drop_proc = Some(format!("proc wait failed: {error}"));
                }
            }
        }

        if out.is_empty() && drop_proc.is_none() && channel_closed {
            drop_proc = Some("proc disconnected".to_string());
        }

        if let Some(reason) = drop_proc {
            let _ = procs.remove(&pb.key);
            return Err(TransportError::Transport(reason));
        }

        Ok(out)
    }

    async fn proc_exec(&self, px: ProcExec) -> Result<Vec<String>, TransportError> {
        let mut it = px.cmd.iter();
        let exe = it
            .next()
            .ok_or_else(|| TransportError::Transport("empty command".into()))?;

        let mut cmd = tokio::process::Command::new(exe);
        for arg in it {
            let _ = cmd.arg(arg);
        }
        let output = cmd
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .output()
            .await
            .map_err(|error| TransportError::Transport(error.to_string()))?;

        if !output.status.success() {
            return Err(TransportError::Transport(format!(
                "proc exited: {}",
                output.status
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(ToString::to_string)
            .collect())
    }

    async fn http_get(&self, get: HttpGet) -> Result<HttpResponse, TransportError> {
        let now = Instant::now();

        {
            let st = self.http.lock().await;
            if let Some(ent) = st.cache.get(&get.key)
                && now < ent.fresh_until
            {
                return Ok(ent.response.clone());
            }
        }

        let client = self
            .clients
            .client_for_host(get.url.host_str().unwrap_or_default(), get.policy.rate)
            .map_err(|e| TransportError::Transport(e.to_string()))?;

        let url_str = get.url.to_string();
        self.log_http(&format!("REQ GET {url_str}"));
        let start = Instant::now();
        let res = client.get(get.url).send().await.map_err(|e| {
            self.log_http(&format!("ERR {e} {url_str}"));
            TransportError::Transport(e.to_string())
        })?;
        let status = res.status().as_u16();
        if !(200..300).contains(&status) {
            self.log_http(&format!(
                "RES {} {}ms {}",
                status,
                start.elapsed().as_millis(),
                url_str
            ));
            return Err(TransportError::Http { status });
        }
        let body = res.bytes().await.map_err(|e| {
            self.log_http(&format!("ERR {e} {url_str}"));
            TransportError::Transport(e.to_string())
        })?;
        self.log_http(&format!(
            "RES {} {}ms {}",
            status,
            start.elapsed().as_millis(),
            url_str
        ));
        let response = HttpResponse { body };

        let mut st = self.http.lock().await;
        let _ = st.cache.insert(
            get.key,
            HttpCacheEntry {
                fresh_until: now + get.policy.cache_fresh_for,
                response: response.clone(),
            },
        );
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::{EffectEngine, ProcBatch, ProcExec, ProcKey, TransportError};
    use std::time::Duration;

    #[tokio::test]
    async fn proc_batch_reports_immediate_exit() {
        let effects = EffectEngine::new();
        let error = effects
            .proc_batch(ProcBatch {
                key: ProcKey::new("immediate-exit"),
                cmd: vec!["sh".to_string(), "-lc".to_string(), "exit 7".to_string()],
                max_lines: 8,
                startup_grace: Duration::from_millis(250),
            })
            .await;
        assert!(
            matches!(&error, Err(TransportError::Transport(message)) if message.contains("proc exited")),
            "unexpected proc result: {error:?}"
        );
    }

    #[tokio::test]
    async fn proc_batch_waits_briefly_for_first_line() {
        let effects = EffectEngine::new();
        let lines = effects
            .proc_batch(ProcBatch {
                key: ProcKey::new("delayed-first-line"),
                cmd: vec![
                    "sh".to_string(),
                    "-lc".to_string(),
                    "sleep 0.05; printf 'alive\\n'; sleep 1".to_string(),
                ],
                max_lines: 8,
                startup_grace: Duration::from_millis(250),
            })
            .await;
        assert!(
            matches!(&lines, Ok(lines) if *lines == vec!["alive".to_string()]),
            "proc should yield its first line: {lines:?}"
        );
    }

    #[tokio::test]
    async fn proc_exec_collects_stdout_lines() {
        let effects = EffectEngine::new();
        let lines = effects
            .proc_exec(ProcExec {
                cmd: vec![
                    "sh".to_string(),
                    "-lc".to_string(),
                    "printf 'alpha\\nbeta\\n'".to_string(),
                ],
            })
            .await;
        assert_eq!(lines, Ok(vec!["alpha".to_string(), "beta".to_string()]));
    }

    #[tokio::test]
    async fn proc_exec_reports_exit_status() {
        let effects = EffectEngine::new();
        let error = effects
            .proc_exec(ProcExec {
                cmd: vec!["sh".to_string(), "-lc".to_string(), "exit 9".to_string()],
            })
            .await;
        assert!(
            matches!(&error, Err(TransportError::Transport(message)) if message.contains("proc exited")),
            "unexpected proc result: {error:?}"
        );
    }
}
