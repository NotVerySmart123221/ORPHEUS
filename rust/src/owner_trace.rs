//! Bounded opt-in diagnostics. The match worker never writes trace output.

use std::{
    ffi::OsString,
    fmt,
    io::{self, BufWriter, Write},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

const CAPACITY: usize = 512;
const FLUSH_INTERVAL: Duration = Duration::from_millis(250);
static TRACE: OnceLock<Option<Trace>> = OnceLock::new();
static PLAYTEST: OnceLock<Option<Trace>> = OnceLock::new();

enum Entry {
    Line(String),
    Flush(SyncSender<()>),
}

struct Trace {
    sender: SyncSender<Entry>,
    dropped: Arc<AtomicU64>,
}

impl Trace {
    fn start(path: Option<OsString>, label: &'static str) -> io::Result<Self> {
        let output: Box<dyn Write + Send> = if let Some(path) = path {
            Box::new(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)?,
            )
        } else {
            Box::new(io::stderr())
        };
        let (sender, receiver) = mpsc::sync_channel(CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&dropped);
        thread::Builder::new()
            .name(format!("orpheus-{label}-trace"))
            .spawn(move || {
                if let Err(error) = write_trace(output, receiver, &counter, label) {
                    eprintln!("{label} trace output stopped: {error}");
                }
            })?;
        Ok(Self { sender, dropped })
    }

    fn record(&self, line: String) {
        if matches!(
            self.sender.try_send(Entry::Line(line)),
            Err(TrySendError::Full(_))
        ) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(crate) fn enabled() -> bool {
    TRACE
        .get_or_init(|| {
            if !std::env::var_os("ORPHEUS_TRACE_OWNER_VOLLEYS").is_some_and(|value| value == "1") {
                return None;
            }
            match Trace::start(std::env::var_os("ORPHEUS_OWNER_TRACE_FILE"), "owner-volley") {
                Ok(trace) => Some(trace),
                Err(error) => {
                    eprintln!("owner trace output unavailable: {error}");
                    None
                }
            }
        })
        .is_some()
}

pub(crate) fn record(line: fmt::Arguments<'_>) {
    if let Some(Some(trace)) = TRACE.get() {
        trace.record(line.to_string());
    }
}

pub(crate) fn start_playtest_capture() {
    PLAYTEST.get_or_init(|| {
        let path = std::env::var_os("ORPHEUS_PLAYTEST_CAPTURE_FILE")?;
        match Trace::start(Some(path), "playtest") {
            Ok(trace) => {
                trace.record("playtest capture=ready".into());
                Some(trace)
            }
            Err(error) => {
                eprintln!("playtest capture unavailable: {error}");
                None
            }
        }
    });
}

pub(crate) fn playtest(line: fmt::Arguments<'_>) {
    if let Some(Some(trace)) = PLAYTEST.get() {
        trace.record(line.to_string());
    }
}

pub(crate) fn playtest_enabled() -> bool {
    PLAYTEST.get().is_some_and(Option::is_some)
}

/// Allow a bounded flush after the match worker has already stopped.
pub(crate) fn flush_after_stop() {
    for trace in [TRACE.get(), PLAYTEST.get()]
        .into_iter()
        .flatten()
        .flatten()
    {
        let (reply, done) = mpsc::sync_channel(1);
        if trace.sender.try_send(Entry::Flush(reply)).is_ok() {
            let _ = done.recv_timeout(2 * FLUSH_INTERVAL);
        }
    }
}

fn write_trace(
    output: impl Write,
    input: Receiver<Entry>,
    dropped: &AtomicU64,
    label: &str,
) -> io::Result<()> {
    let mut writer = BufWriter::new(output);
    let mut due = Instant::now() + FLUSH_INTERVAL;
    loop {
        let response = input.recv_timeout(due.saturating_duration_since(Instant::now()));
        let mut flush = None;
        let mut stopped = false;
        match response {
            Ok(Entry::Line(line)) => writeln!(writer, "{line}")?,
            Ok(Entry::Flush(reply)) => flush = Some(reply),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => stopped = true,
        }
        if stopped || flush.is_some() || Instant::now() >= due {
            let dropped = dropped.swap(0, Ordering::Relaxed);
            if dropped != 0 {
                writeln!(writer, "{label} trace dropped={dropped}")?;
            }
            writer.flush()?;
            due = Instant::now() + FLUSH_INTERVAL;
        }
        if let Some(reply) = flush {
            let _ = reply.try_send(());
        }
        if stopped {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_queue_keeps_oldest_line_and_reports_overflow_without_waiting() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let dropped = Arc::new(AtomicU64::new(0));
        let trace = Trace {
            sender,
            dropped: Arc::clone(&dropped),
        };
        trace.record("first line".into());
        trace.record("overflow".into());
        drop(trace);

        let mut output = Vec::new();
        write_trace(&mut output, receiver, &dropped, "owner-volley").unwrap();
        assert_eq!(output, b"first line\nowner-volley trace dropped=1\n");
    }
}
