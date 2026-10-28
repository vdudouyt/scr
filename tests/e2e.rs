//! End-to-end: a real client on a real PTY, a real daemon, detach/reattach,
//! tabs, and the `kill -9` cleanup promise.

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Tests only need the process gone, so go through `kill` and stay safe.
fn kill9(pid: i32) {
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
}

struct Env {
    dir: PathBuf,
    name: String,
}

impl Env {
    fn new(tag: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let name = format!("e2e-{tag}-{}-{stamp}", std::process::id());
        let dir = std::env::temp_dir().join(format!(
            "scr-e2e-{}-{stamp}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Env { dir, name }
    }

    fn command(&self) -> CommandBuilder {
        let exe = env!("CARGO_BIN_EXE_scr");
        let mut cmd = CommandBuilder::new(exe);
        cmd.env("SCR_RUNTIME_DIR", self.dir.as_os_str());
        cmd.env("TERM", "xterm-256color");
        cmd
    }

    fn scr(&self, args: &[&str]) -> std::process::Output {
        let exe = env!("CARGO_BIN_EXE_scr");
        Command::new(exe)
            .args(args)
            .env("SCR_RUNTIME_DIR", &self.dir)
            .env("TERM", "xterm-256color")
            .output()
            .unwrap()
    }

    fn list(&self) -> String {
        let out = self.scr(&["-ls"]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        // The daemon is intentionally detached from the client; make sure a
        // failed assertion cannot leave it behind.
        if let Ok(out) = Command::new("pgrep")
            .args([
                "-f",
                &format!("{} --server {}", env!("CARGO_BIN_EXE_scr"), self.name),
            ])
            .output()
        {
            for pid in String::from_utf8_lossy(&out.stdout).split_whitespace() {
                if let Ok(pid) = pid.parse::<i32>() {
                    kill9(pid);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Ui {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    rx: Receiver<Vec<u8>>,
    parser: vt100::Parser,
    raw: Vec<u8>,
    /// The pty's modes before the client touched them, as a stable debug form.
    sane_termios: String,
}

impl Ui {
    fn spawn(env: &Env, args: &[&str]) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let sane_termios = format!("{:?}", pair.master.get_termios());
        let mut cmd = env.command();
        cmd.args(args);
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match std::io::Read::read(&mut reader, &mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        Ui {
            child,
            writer,
            master: pair.master,
            rx,
            parser: vt100::Parser::new(24, 80, 2000),
            raw: Vec::new(),
            sane_termios,
        }
    }

    fn termios(&self) -> String {
        format!("{:?}", self.master.get_termios())
    }

    fn pump(&mut self) {
        while let Ok(chunk) = self.rx.recv_timeout(Duration::from_millis(20)) {
            self.parser.process(&chunk);
            self.raw.extend_from_slice(&chunk);
        }
    }

    fn wait_for(&mut self, needle: &str, secs: u64) {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            self.pump();
            if self.parser.screen().contents().contains(needle) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "never saw {needle:?}; screen was:\n{}",
            self.parser.screen().contents()
        );
    }

    fn write(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    fn ctrl(&mut self, c: char) {
        self.write(&[(c as u8) & 0x1f]);
    }

    fn wait_exit(&mut self) {
        let _ = self.child.wait();
    }
}

#[test]
fn detach_reattach_tabs_and_kill_cleanup() {
    let env = Env::new("flow");
    let name = env.name.clone();

    let mut ui = Ui::spawn(&env, &["-S", &name]);
    // The shell sets its own OSC title, so the tab number is what stays fixed.
    ui.wait_for("1:", 8);
    ui.write(b"echo E2E_ONE\r");
    ui.wait_for("E2E_ONE", 8);

    // Ctrl+A c: a second tab, active.
    ui.ctrl('a');
    ui.write(b"c");
    ui.wait_for("2:", 8);
    ui.write(b"echo E2E_TWO\r");
    ui.wait_for("E2E_TWO", 8);

    // Alt+1 switches back without the dialog.
    ui.write(b"\x1b1");
    ui.wait_for("E2E_ONE", 8);

    // Ctrl+A d: detached, client gone, daemon alive.
    ui.ctrl('a');
    ui.write(b"d");
    ui.wait_exit();
    let listed = env.list();
    assert!(listed.contains(&name), "listing was:\n{listed}");
    assert!(listed.contains("detached"), "listing was:\n{listed}");
    assert!(listed.contains("2"), "two tabs should survive: {listed}");

    // Reattach and see the first tab's output again.
    let mut ui = Ui::spawn(&env, &["-r", &name]);
    ui.wait_for("E2E_ONE", 8);
    ui.wait_for("2:", 8);

    // Close both tabs; the last close ends the session.
    ui.ctrl('a');
    ui.write(b"k");
    ui.ctrl('a');
    ui.write(b"k");
    ui.wait_exit();
    let listed = env.list();
    assert!(
        listed.contains("No sessions"),
        "session should be gone: {listed}"
    );

    // A second session, killed with SIGKILL, must vanish from the listing.
    let env2 = Env::new("kill");
    let name2 = env2.name.clone();
    let mut ui = Ui::spawn(&env2, &["-S", &name2]);
    ui.wait_for("1:", 8);
    ui.ctrl('a');
    ui.write(b"d");
    ui.wait_exit();
    assert!(env2.list().contains(&name2));

    let out = Command::new("pgrep")
        .args([
            "-f",
            &format!("{} --server {name2}", env!("CARGO_BIN_EXE_scr")),
        ])
        .output()
        .unwrap();
    let pid: i32 = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .expect("server pid")
        .parse()
        .unwrap();
    kill9(pid);
    std::thread::sleep(Duration::from_millis(150));
    let listed = env2.list();
    assert!(
        listed.contains("No sessions"),
        "killed session should be pruned: {listed}"
    );
    assert!(!env2.dir.join(format!("{name2}.sock")).exists());
    assert!(!env2.dir.join(format!("{name2}.lock")).exists());
}

#[test]
fn a_renamed_tab_outranks_the_programs_title() {
    let env = Env::new("rename");
    let name = env.name.clone();
    let mut ui = Ui::spawn(&env, &["-S", &name]);
    ui.wait_for("1:", 8);

    ui.ctrl('a');
    ui.write(b"\"");
    ui.wait_for("e rename", 8);
    ui.write(b"e");
    for _ in 0..200 {
        ui.write(&[0x7f]);
    }
    ui.write(b"mine\r");
    ui.wait_for("1:mine", 8);

    // A new prompt asks the shell to set its own title again; the manual name
    // must win.
    ui.write(b"\r");
    std::thread::sleep(Duration::from_millis(700));
    ui.pump();
    let header = ui
        .parser
        .screen()
        .contents()
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    assert!(header.contains("1:mine"), "header was {header:?}");
    assert!(
        !header.contains('@'),
        "the program's title leaked into the header: {header:?}"
    );

    // And it survives detach and reattach.
    ui.ctrl('a');
    ui.write(b"d");
    ui.wait_exit();
    let mut ui = Ui::spawn(&env, &["-r", &name]);
    ui.wait_for("1:mine", 8);
    ui.ctrl('a');
    ui.write(b"k");
    ui.wait_exit();
}

#[test]
fn sigkill_restores_the_terminal_through_the_guardian() {
    let env = Env::new("guard");
    let name = env.name.clone();
    let mut ui = Ui::spawn(&env, &["-S", &name]);
    ui.wait_for("1:", 8);
    let pid = ui.child.process_id().expect("client pid") as i32;
    kill9(pid);
    let deadline = Instant::now() + Duration::from_secs(5);
    let marker = b"\x1b[?1049l";
    while Instant::now() < deadline {
        ui.pump();
        if ui.raw.windows(marker.len()).any(|w| w == marker) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        ui.raw.windows(marker.len()).any(|w| w == marker),
        "guardian never left the alternate screen"
    );
    assert!(
        ui.raw.windows(8).any(|w| w == b"\x1b[?1003l"),
        "guardian never disabled mouse reporting"
    );
    assert_eq!(
        ui.sane_termios,
        ui.termios(),
        "guardian never restored the terminal modes"
    );
    let _ = ui.child.wait();
}
