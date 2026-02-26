use codexmanager_core::storage::now_ts;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_TRACE_QUEUE_CAPACITY: usize = 2048;
const TRACE_FLUSH_WAIT_TIMEOUT_MS: u64 = 200;
const ENV_TRACE_QUEUE_CAPACITY: &str = "CODEXMANAGER_TRACE_QUEUE_CAPACITY";

static TRACE_WRITER: OnceLock<TraceAsyncWriter> = OnceLock::new();
static TRACE_SEQ: AtomicU64 = AtomicU64::new(1);

enum TraceCommand {
    Append {
        line: String,
        flush: bool,
        ack: Option<SyncSender<()>>,
    },
    ResetPath(PathBuf),
}

struct TraceAsyncWriter {
    tx: SyncSender<TraceCommand>,
    dropped: AtomicU64,
    queue_capacity: usize,
}

impl TraceAsyncWriter {
    fn new(path: PathBuf) -> Self {
        let queue_capacity = trace_queue_capacity();
        let (tx, rx) = mpsc::sync_channel::<TraceCommand>(queue_capacity);
        let _ = thread::Builder::new()
            .name("gateway-trace-writer".to_string())
            .spawn(move || trace_writer_loop(rx, TraceFileWriter::new(path)));
        Self {
            tx,
            dropped: AtomicU64::new(0),
            queue_capacity,
        }
    }

    fn append_line(&self, line: String, flush: bool) {
        if flush {
            let (ack_tx, ack_rx) = mpsc::sync_channel(0);
            if self
                .tx
                .send(TraceCommand::Append {
                    line,
                    flush: true,
                    ack: Some(ack_tx),
                })
                .is_err()
            {
                log::warn!("gateway trace enqueue failed: writer channel closed");
                return;
            }
            let _ = ack_rx.recv_timeout(Duration::from_millis(TRACE_FLUSH_WAIT_TIMEOUT_MS));
            return;
        }

        match self.tx.try_send(TraceCommand::Append {
            line,
            flush: false,
            ack: None,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                let dropped = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                if dropped == 1 || dropped % 1024 == 0 {
                    log::warn!(
                        "gateway trace queue full; dropped_lines={}, capacity={}",
                        dropped,
                        self.queue_capacity
                    );
                }
            }
            Err(TrySendError::Disconnected(_)) => {
                log::warn!("gateway trace enqueue failed: writer channel closed");
            }
        }
    }

    fn reset_path(&self, path: PathBuf) {
        if self.tx.send(TraceCommand::ResetPath(path)).is_err() {
            log::warn!("gateway trace reset-path failed: writer channel closed");
        }
    }
}

struct TraceFileWriter {
    path: PathBuf,
    writer: Option<BufWriter<File>>,
}

impl TraceFileWriter {
    fn new(path: PathBuf) -> Self {
        Self { path, writer: None }
    }

    fn reset_path(&mut self, next_path: PathBuf) {
        if self.path == next_path {
            return;
        }
        self.path = next_path;
        self.writer = None;
    }

    fn append_line(&mut self, line: &str, flush: bool) -> std::io::Result<()> {
        let writer = self.ensure_open_writer()?;
        writeln!(writer, "{line}")?;
        if flush {
            writer.flush()?;
        }
        Ok(())
    }

    fn ensure_open_writer(&mut self) -> std::io::Result<&mut BufWriter<File>> {
        if self.writer.is_none() {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            self.writer = Some(BufWriter::new(file));
        }
        Ok(self.writer.as_mut().expect("writer should be initialized"))
    }
}

fn trace_file_path_from_env() -> PathBuf {
    if let Ok(db_path) = std::env::var("CODEXMANAGER_DB_PATH") {
        let path = PathBuf::from(db_path);
        if let Some(parent) = path.parent() {
            return parent.join("gateway-trace.log");
        }
    }
    PathBuf::from("gateway-trace.log")
}

fn sanitize_text(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn short_fingerprint(value: &str) -> String {
    let mut hash: u64 = 14695981039346656037;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("{hash:016x}")
}

fn append_trace_line(line: String, flush: bool) {
    trace_writer().append_line(line, flush);
}

fn trace_writer() -> &'static TraceAsyncWriter {
    TRACE_WRITER.get_or_init(|| TraceAsyncWriter::new(trace_file_path_from_env()))
}

fn trace_writer_loop(rx: Receiver<TraceCommand>, mut writer: TraceFileWriter) {
    while let Ok(command) = rx.recv() {
        match command {
            TraceCommand::Append { line, flush, ack } => {
                if let Err(err) = writer.append_line(&line, flush) {
                    log::warn!(
                        "gateway trace write failed: path={}, err={}",
                        writer.path.display(),
                        err
                    );
                    writer.writer = None;
                }
                if let Some(ack) = ack {
                    let _ = ack.send(());
                }
            }
            TraceCommand::ResetPath(path) => writer.reset_path(path),
        }
    }
}

fn trace_queue_capacity() -> usize {
    std::env::var(ENV_TRACE_QUEUE_CAPACITY)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_TRACE_QUEUE_CAPACITY)
}

pub(super) fn reload_from_env() {
    let path = trace_file_path_from_env();
    trace_writer().reset_path(path);
}

pub(crate) fn next_trace_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_millis())
        .unwrap_or(0);
    let seq = TRACE_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("trc_{millis}_{seq:x}")
}

pub(crate) fn log_request_start(
    trace_id: &str,
    key_id: &str,
    method: &str,
    path: &str,
    model: Option<&str>,
    reasoning: Option<&str>,
    is_stream: bool,
    protocol_type: &str,
) {
    let ts = now_ts();
    let model = model.unwrap_or("-");
    let reasoning = reasoning.unwrap_or("-");
    let line = format!(
        "ts={ts} event=REQUEST_START trace_id={} key_id={} method={} path={} model={} reasoning={} stream={} protocol={}",
        sanitize_text(trace_id),
        sanitize_text(key_id),
        sanitize_text(method),
        sanitize_text(path),
        sanitize_text(model),
        sanitize_text(reasoning),
        is_stream,
        sanitize_text(protocol_type),
    );
    append_trace_line(line, false);
}

pub(crate) fn log_request_body_preview(trace_id: &str, body: &[u8]) {
    let ts = now_ts();
    let preview_max_bytes = super::trace_body_preview_max_bytes();
    if preview_max_bytes == 0 {
        let line = format!(
            "ts={ts} event=REQUEST_BODY trace_id={} len={} preview_bytes=0",
            sanitize_text(trace_id),
            body.len(),
        );
        append_trace_line(line, false);
        return;
    }

    let preview_len = preview_max_bytes.min(body.len());
    let preview_raw = String::from_utf8_lossy(&body[..preview_len]);
    let preview = preview_raw
        .chars()
        .filter(|ch| *ch != '\r' && *ch != '\n' && *ch != '\t')
        .collect::<String>();
    let line = format!(
        "ts={ts} event=REQUEST_BODY trace_id={} len={} preview_bytes={} truncated={} preview={}",
        sanitize_text(trace_id),
        body.len(),
        preview_len,
        body.len() > preview_len,
        sanitize_text(preview.as_str()),
    );
    append_trace_line(line, false);
}

pub(crate) fn log_request_gate_wait(trace_id: &str, key_id: &str, path: &str, model: Option<&str>) {
    let ts = now_ts();
    let line = format!(
        "ts={ts} event=REQUEST_GATE_WAIT trace_id={} key_id={} path={} model={}",
        sanitize_text(trace_id),
        sanitize_text(key_id),
        sanitize_text(path),
        sanitize_text(model.unwrap_or("-")),
    );
    append_trace_line(line, false);
}

pub(crate) fn log_request_gate_acquired(
    trace_id: &str,
    key_id: &str,
    path: &str,
    model: Option<&str>,
    wait_ms: u128,
) {
    let ts = now_ts();
    let line = format!(
        "ts={ts} event=REQUEST_GATE_ACQUIRED trace_id={} key_id={} path={} model={} wait_ms={}",
        sanitize_text(trace_id),
        sanitize_text(key_id),
        sanitize_text(path),
        sanitize_text(model.unwrap_or("-")),
        wait_ms,
    );
    append_trace_line(line, false);
}

pub(crate) fn log_request_gate_skip(trace_id: &str, reason: &str) {
    let ts = now_ts();
    let line = format!(
        "ts={ts} event=REQUEST_GATE_SKIP trace_id={} reason={}",
        sanitize_text(trace_id),
        sanitize_text(reason),
    );
    append_trace_line(line, false);
}

pub(crate) fn log_candidate_start(
    trace_id: &str,
    idx: usize,
    total: usize,
    account_id: &str,
    strip_session_affinity: bool,
) {
    let ts = now_ts();
    let line = format!(
        "ts={ts} event=CANDIDATE_START trace_id={} candidate={}/{} account_id={} strip_session_affinity={}",
        sanitize_text(trace_id),
        idx + 1,
        total,
        sanitize_text(account_id),
        strip_session_affinity,
    );
    append_trace_line(line, false);
}

pub(crate) fn log_candidate_pool(
    trace_id: &str,
    key_id: &str,
    strategy: &str,
    candidates: &[String],
) {
    let ts = now_ts();
    let ordered = if candidates.is_empty() {
        "-".to_string()
    } else {
        candidates
            .iter()
            .map(|item| sanitize_text(item))
            .collect::<Vec<_>>()
            .join(",")
    };
    let line = format!(
        "ts={ts} event=CANDIDATE_POOL trace_id={} key_id={} strategy={} ordered_candidates={}",
        sanitize_text(trace_id),
        sanitize_text(key_id),
        sanitize_text(strategy),
        ordered,
    );
    append_trace_line(line, false);
}

pub(crate) fn log_candidate_skip(
    trace_id: &str,
    idx: usize,
    total: usize,
    account_id: &str,
    reason: &str,
) {
    let ts = now_ts();
    let line = format!(
        "ts={ts} event=CANDIDATE_SKIP trace_id={} candidate={}/{} account_id={} reason={}",
        sanitize_text(trace_id),
        idx + 1,
        total,
        sanitize_text(account_id),
        sanitize_text(reason),
    );
    append_trace_line(line, false);
}

pub(crate) fn log_attempt_result(
    trace_id: &str,
    account_id: &str,
    upstream_url: Option<&str>,
    status_code: u16,
    error: Option<&str>,
) {
    let ts = now_ts();
    let url = upstream_url.unwrap_or("-");
    let error = error.unwrap_or("-");
    let line = format!(
        "ts={ts} event=ATTEMPT_RESULT trace_id={} account_id={} status={} upstream_url={} error={}",
        sanitize_text(trace_id),
        sanitize_text(account_id),
        status_code,
        sanitize_text(url),
        sanitize_text(error),
    );
    append_trace_line(line, false);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn log_attempt_profile(
    trace_id: &str,
    account_id: &str,
    candidate_index: usize,
    total: usize,
    strip_session_affinity: bool,
    has_incoming_session: bool,
    has_incoming_turn_state: bool,
    has_incoming_conversation: bool,
    prompt_cache_key: Option<&str>,
    request_shape: Option<&str>,
    body_len: usize,
    body_model: Option<&str>,
) {
    let ts = now_ts();
    let prompt_cache_key_fp = prompt_cache_key
        .map(short_fingerprint)
        .unwrap_or_else(|| "-".to_string());
    let session_source = if strip_session_affinity {
        "failover_regen"
    } else if has_incoming_session {
        "incoming_header"
    } else if prompt_cache_key.is_some() {
        "prompt_cache_key"
    } else {
        "generated"
    };
    let request_shape = request_shape.unwrap_or("-");
    let line = format!(
        "ts={ts} event=ATTEMPT_PROFILE trace_id={} account_id={} candidate={}/{} strip_session_affinity={} session_source={} has_turn_state={} has_conversation={} prompt_cache_key_fp={} request_shape={} body_len={} body_model={}",
        sanitize_text(trace_id),
        sanitize_text(account_id),
        candidate_index + 1,
        total,
        strip_session_affinity,
        session_source,
        has_incoming_turn_state,
        has_incoming_conversation,
        prompt_cache_key_fp,
        sanitize_text(request_shape),
        body_len,
        sanitize_text(body_model.unwrap_or("-")),
    );
    append_trace_line(line, false);
}

pub(crate) fn log_request_final(
    trace_id: &str,
    status_code: u16,
    final_account_id: Option<&str>,
    upstream_url: Option<&str>,
    error: Option<&str>,
    elapsed_ms: u128,
) {
    let ts = now_ts();
    let account_id = final_account_id.unwrap_or("-");
    let upstream_url = upstream_url.unwrap_or("-");
    let error = error.unwrap_or("-");
    let line = format!(
        "ts={ts} event=REQUEST_FINAL trace_id={} status={} account_id={} upstream_url={} elapsed_ms={} error={}",
        sanitize_text(trace_id),
        status_code,
        sanitize_text(account_id),
        sanitize_text(upstream_url),
        elapsed_ms,
        sanitize_text(error),
    );
    append_trace_line(line, true);
}
