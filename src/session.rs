//! Session discovery, liveness and daemon startup.
//!
//! A session is a Unix socket plus a lock file held by its daemon. The lock is
//! what makes `kill -9` show up immediately: the kernel drops it, so the next
//! listing knows the socket is stale without trusting a pid or a timestamp.

use crate::proto::{self, ClientMsg, SessionInfo, TAG_INFO};
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct SessionLock {
    file: File,
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        // Leave the lock file in place. Removing it after unlocking opens a
        // race where two new daemons can each believe they hold it; a stale
        // file is harmless and `list_sessions` prunes it.
        let _ = self.file.unlock();
    }
}

pub fn runtime_dir() -> Result<PathBuf> {
    let dir = if let Some(d) = std::env::var_os("SCR_RUNTIME_DIR") {
        PathBuf::from(d)
    } else if let Some(d) = dirs::runtime_dir() {
        d.join("scr")
    } else if let Some(home) = dirs::home_dir() {
        home.join(".scr")
    } else {
        std::env::temp_dir().join("scr")
    };
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    Ok(dir)
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 {
        bail!("session name must be 1–64 characters");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        bail!("session names may contain only letters, digits, '.', '_' and '-'");
    }
    Ok(())
}

fn sock_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.sock"))
}

fn lock_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.lock"))
}

pub fn log_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.log"))
}

/// Hold the session's exclusive lock for the daemon's lifetime.
pub fn acquire_lock(name: &str) -> Result<SessionLock> {
    let dir = runtime_dir()?;
    let path = lock_path(&dir, name);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    file.try_lock()
        .map_err(|_| anyhow::anyhow!("session {name} is already running"))?;
    Ok(SessionLock { file })
}

fn lock_held(path: &Path) -> bool {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(_) => true,
    }
}

pub fn connect(name: &str) -> Result<UnixStream> {
    let dir = runtime_dir()?;
    UnixStream::connect(sock_path(&dir, name))
        .with_context(|| format!("connect to session {name}"))
}

/// Ask a session who it is. Returns `None` when it is gone, and prunes its
/// socket and lock if nothing holds the lock any more.
pub fn probe(name: &str) -> Option<SessionInfo> {
    let dir = runtime_dir().ok()?;
    let sock = sock_path(&dir, name);
    let mut stream = match UnixStream::connect(&sock) {
        Ok(s) => s,
        Err(_) => {
            prune_if_dead(&dir, name);
            return None;
        }
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    if proto::write_json(&mut stream, proto::TAG_JSON, &ClientMsg::Probe).is_err() {
        drop(stream);
        prune_if_dead(&dir, name);
        return None;
    }
    match proto::read_frame(&mut stream) {
        Ok((TAG_INFO, body)) => serde_json::from_slice::<SessionInfo>(&body).ok(),
        Ok(_) => None,
        Err(_) => {
            drop(stream);
            prune_if_dead(&dir, name);
            None
        }
    }
}

fn prune_if_dead(dir: &Path, name: &str) {
    let lock = lock_path(dir, name);
    if lock_held(&lock) {
        // Starting up, or busy speaking to another client.
        return;
    }
    let _ = fs::remove_file(sock_path(dir, name));
    let _ = fs::remove_file(&lock);
}

pub fn list_sessions() -> Result<Vec<SessionInfo>> {
    let dir = runtime_dir()?;
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(stem) = name.strip_suffix(".sock") {
            names.insert(stem.to_string());
        } else if let Some(stem) = name.strip_suffix(".lock") {
            names.insert(stem.to_string());
        }
    }
    let mut out = Vec::new();
    for name in names {
        if !validate_name(&name).is_ok() {
            continue;
        }
        if let Some(info) = probe(&name) {
            out.push(info);
        }
    }
    out.sort_by_key(|s| s.created);
    Ok(out)
}

pub fn default_name() -> String {
    let mut name = std::process::id().to_string();
    let dir = runtime_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut n = 1;
    while lock_held(&lock_path(&dir, &name)) {
        name = format!("{}-{n}", std::process::id());
        n += 1;
    }
    name
}

/// Start a session daemon and wait until it answers a probe.
pub fn spawn_server(name: &str, command: &[String]) -> Result<()> {
    validate_name(name)?;
    let dir = runtime_dir()?;
    if lock_held(&lock_path(&dir, name)) {
        bail!("session {name} is already running");
    }
    let exe = std::env::current_exe().context("locate the scr binary")?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(log_path(&dir, name))
        .with_context(|| format!("open session log for {name}"))?;
    let log2 = log.try_clone()?;
    let mut child = Command::new(exe)
        .arg("--server")
        .arg(name)
        .args(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log2))
        // A group of its own keeps terminal-generated SIGHUP/SIGINT/SIGQUIT
        // away from a daemon that holds no terminal descriptors.
        .process_group(0)
        .spawn()
        .context("start session daemon")?;

    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        if let Some(info) = probe(name) {
            let _ = info;
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            let text = fs::read_to_string(log_path(&dir, name)).unwrap_or_default();
            let tail = text.lines().rev().take(8).collect::<Vec<_>>();
            bail!(
                "session daemon exited with {status}{}",
                if tail.is_empty() {
                    String::new()
                } else {
                    format!(":\n{}", tail.into_iter().rev().collect::<Vec<_>>().join("\n"))
                }
            );
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let _ = child.kill();
    bail!("session daemon did not start within 4 seconds");
}

/// Write a fatal daemon error where the operator can find it.
pub fn log_error(name: &str, error: &anyhow::Error) {
    if let Ok(dir) = runtime_dir() {
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(log_path(&dir, name))
        {
            let _ = writeln!(f, "scr: {error:#}");
        }
    }
}

pub fn session_name_from_arg(arg: Option<String>) -> Result<String> {
    match arg {
        Some(name) => {
            validate_name(&name)?;
            Ok(name)
        }
        None => Ok(default_name()),
    }
}
