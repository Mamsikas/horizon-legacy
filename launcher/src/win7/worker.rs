//! Each form invokes the existing CLI as a worker, never a shell or another business backend.
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};

pub enum Event {
    Line(String),
}

pub struct Worker {
    child: Child,
    pub events: Receiver<Event>,
    input: SyncSender<String>,
    pub handoff_allowed: bool,
    cancel: PathBuf,
}

impl Worker {
    pub fn start(args: &[OsString]) -> Result<Self, String> {
        let cancel = horizon_launcher::backend::data_dir()
            .join("work")
            .join(format!("native-cancel-{}", std::process::id()));
        std::fs::create_dir_all(cancel.parent().unwrap()).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&cancel);
        let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        command
            .args(args)
            .env("HORIZON_LAUNCHER_CANCEL_FILE", &cancel)
            .env("HORIZON_LAUNCHER_NATIVE_PROGRESS", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if std::env::var_os("HORIZON_PREPARE_JOBS").is_none() {
            command.env("HORIZON_PREPARE_JOBS", "1");
        }
        if std::env::var_os("HORIZON_PREPARE_MAX_HEAP_MIB").is_none() {
            command.env("HORIZON_PREPARE_MAX_HEAP_MIB", "512");
        }
        horizon_launcher::process::hide_console(&mut command);
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let (tx, events) = mpsc::sync_channel(256);
        read_lines(child.stdout.take().unwrap(), tx.clone());
        read_lines(child.stderr.take().unwrap(), tx);
        let mut stdin = child.stdin.take().unwrap();
        let (input, lines) = mpsc::sync_channel::<String>(16);
        std::thread::spawn(move || {
            for line in lines {
                if writeln!(stdin, "{line}")
                    .and_then(|_| stdin.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        let handoff_allowed = args.first().is_some_and(|v| v == "legacy")
            && args.get(1).is_some_and(|v| {
                matches!(
                    v.to_str(),
                    Some("storage-move" | "storage-retry" | "storage-default")
                )
            });
        Ok(Self {
            child,
            events,
            input,
            handoff_allowed,
            cancel,
        })
    }

    pub fn send(&self, line: String) -> Result<(), String> {
        self.input.try_send(line).map_err(|e| e.to_string())
    }

    /// Cooperative: downloads leave checked destinations intact. Other CLI operations finish
    /// at their existing safe boundary; never terminate an installer or a world save midway.
    pub fn cancel(&self) -> Result<(), String> {
        std::fs::write(&self.cancel, b"cancel").map_err(|e| e.to_string())
    }

    pub fn poll(&mut self) -> Option<Result<(), String>> {
        match self.child.try_wait() {
            Ok(Some(status)) if status.success() => Some(Ok(())),
            Ok(Some(status)) => Some(Err(status.to_string())),
            Ok(None) => None,
            Err(e) => Some(Err(e.to_string())),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.cancel);
    }
}

fn read_lines(stream: impl std::io::Read + Send + 'static, tx: SyncSender<Event>) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        while let Ok(Some(line)) = bounded_line(&mut reader) {
            if tx.send(Event::Line(line)).is_err() {
                break;
            }
        }
    });
}

// Consume oversized output without ever allocating its full length. A line may be cut through
// a UTF-8 character; lossy decoding is appropriate for subprocess diagnostic output.
fn bounded_line(reader: &mut impl BufRead) -> std::io::Result<Option<String>> {
    let mut line = Vec::with_capacity(256);
    let mut any = false;
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            return Ok(any.then(|| {
                String::from_utf8_lossy(&line)
                    .trim_end_matches('\r')
                    .to_owned()
            }));
        }
        any = true;
        let end = bytes.iter().position(|&b| b == b'\n');
        let count = end.unwrap_or(bytes.len());
        let keep = count.min(8192usize.saturating_sub(line.len()));
        line.extend_from_slice(&bytes[..keep]);
        reader.consume(count + usize::from(end.is_some()));
        if end.is_some() {
            while line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
        }
    }
}

/// Bounded in both lines and UTF-8 bytes, even when a child emits huge lines.
pub struct Tail {
    lines: VecDeque<String>,
    bytes: usize,
}

impl Tail {
    pub fn new() -> Self {
        Self {
            lines: VecDeque::new(),
            bytes: 0,
        }
    }
    pub fn push(&mut self, mut line: String) {
        if line.len() > 8192 {
            let mut end = 8192;
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            line.truncate(end);
        }
        self.bytes += line.len();
        self.lines.push_back(line);
        while self.lines.len() > 2000 || self.bytes > 128 * 1024 {
            self.bytes -= self.lines.pop_front().unwrap().len();
        }
    }
    pub fn text(&self) -> String {
        self.lines.iter().cloned().collect::<Vec<_>>().join("\r\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_child_output_is_consumed_without_losing_the_next_line() {
        let input = format!("{}\nследующая строка\r\nlast", "я".repeat(40_000));
        let mut reader = BufReader::with_capacity(17, input.as_bytes());
        let line = bounded_line(&mut reader).unwrap().unwrap();
        assert!(line.len() <= 8195);
        assert_eq!(
            bounded_line(&mut reader).unwrap().unwrap(),
            "следующая строка"
        );
        assert_eq!(bounded_line(&mut reader).unwrap().unwrap(), "last");
        assert!(bounded_line(&mut reader).unwrap().is_none());
    }

    #[test]
    fn log_tail_keeps_recent_output_under_both_limits() {
        let mut tail = Tail::new();
        for i in 0..2500 {
            tail.push(format!("{i}: {}", "x".repeat(500)));
        }
        let text = tail.text();
        assert!(text.ends_with(&format!("2499: {}", "x".repeat(500))));
        assert!(!text.starts_with("0:"));
        assert!(text.len() < 132 * 1024);
    }
}
