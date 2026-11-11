//! A bounded request/reply channel to one initialized exporter JVM.
use crate::*;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{self, Receiver};

pub(crate) struct Worker {
    child: Child,
    input: ChildStdin,
    replies: Receiver<std::io::Result<String>>,
    java: PathBuf,
    log_path: PathBuf,
}

impl Worker {
    pub(crate) fn new(run: &Run, name: &str, classes: &Path, cp: &str) -> Result<Self, String> {
        java::cancelled()?;
        let log_path = run.local.join(format!("{name}-worker.log"));
        let log = run.log_file(&log_path)?;
        let mut child = java::command(&run.jdk.java)
            .args(java::BATCH_OPTIONS)
            .args(java::bounded_args(&[
                "-Xmx2G".into(),
                "-cp".into(),
                format!(
                    "{}{}{}",
                    classes.display(),
                    if cfg!(windows) { ";" } else { ":" },
                    cp
                ),
                "dev.horizonlab.parity.PrepareWorker".into(),
            ])?)
            // Vanilla's code logs relative to it (logs/): in the scratch, as `Run::run`.
            .current_dir(&run.local)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log)
            .spawn()
            .map_err(|e| e.to_string())?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (send, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input,
            replies,
            java: run.jdk.java.clone(),
            log_path,
        })
    }

    pub(crate) fn run(&mut self, main: &str, args: &[String], log: &Path) -> Result<(), String> {
        self.request(main, args, log)
            .map_err(|error| java::export_failure(error, &self.java, &self.log_path))
    }

    fn request(&mut self, main: &str, args: &[String], log: &Path) -> Result<(), String> {
        let task = Value::Array(vec![
            Value::Str(arg(log)),
            Value::Str(main.into()),
            Value::Array(args.iter().cloned().map(Value::Str).collect()),
        ]);
        writeln!(self.input, "{}", pyjson::dumps(&task, &pyjson::COMPACT))
            .map_err(|e| e.to_string())?;
        self.input.flush().map_err(|e| e.to_string())?;
        let started = std::time::Instant::now();
        let reply = loop {
            java::cancelled()?;
            match self
                .replies
                .recv_timeout(std::time::Duration::from_millis(200))
            {
                Ok(reply) => break reply.map_err(|e| e.to_string())?,
                Err(mpsc::RecvTimeoutError::Timeout) if started.elapsed().as_secs() < 3600 => {}
                Err(error) => return Err(format!("Exporter JVM stopped or timed out: {error}")),
            }
        };
        match pyjson::loads(&reply)?.as_str() {
            Some("") => Ok(()),
            _ => Err(format!("Exporter {main} failed: {reply}")),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
