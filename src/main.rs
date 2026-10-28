mod app;
mod cli;
mod client;
mod picker;
mod proto;
mod server;
mod session;
mod terminal;
mod term;
mod ui;

use anyhow::{Context, Result, bail};
use cli::Command;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("scr: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Hidden fallback used when the `scr-guard` helper binary is unavailable.
    if args.len() == 1 && args[0] == terminal::GUARD_ARG {
        terminal::guard_main();
        return Ok(());
    }
    let command = cli::parse(args)?;
    match command {
        Command::Help => {
            print!("{}", cli::USAGE);
            Ok(())
        }
        Command::Version => {
            println!("scr {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::Server { name, command } => {
            if let Err(e) = server::run(name.clone(), command) {
                session::log_error(&name, &e);
                return Err(e);
            }
            Ok(())
        }
        Command::New { name, command } => {
            let name = session::session_name_from_arg(name)?;
            session::spawn_server(&name, &command)?;
            client::attach(&name)
        }
        Command::Resume { name } => resume(name),
        Command::List => list(),
        Command::Detach { name, then_attach } => detach(name, then_attach),
    }
}

fn resume(name: Option<String>) -> Result<()> {
    let sessions = session::list_sessions()?;
    if let Some(name) = name {
        let info = session::probe(&name)
            .with_context(|| format!("no session called {name}"))?;
        if info.attached {
            bail!(
                "session {name} is attached; detach it with: scr -d {name}"
            );
        }
        return client::attach(&name);
    }
    match sessions.len() {
        0 => bail!("no sessions to resume"),
        1 => {
            let s = &sessions[0];
            if s.attached {
                bail!(
                    "session {} is attached; detach it with: scr -d {}",
                    s.name,
                    s.name
                );
            }
            client::attach(&s.name)
        }
        _ => {
            let Some(name) = picker::pick(sessions)? else {
                return Ok(());
            };
            client::attach(&name)
        }
    }
}

fn list() -> Result<()> {
    let sessions = session::list_sessions()?;
    if sessions.is_empty() {
        println!("No sessions.");
        return Ok(());
    }
    println!(
        "{:<24} {:<10} {:>4}  {}",
        "NAME", "STATE", "TABS", "START DIRECTORY"
    );
    for s in sessions {
        println!(
            "{:<24} {:<10} {:>4}  {}",
            s.name,
            if s.attached { "attached" } else { "detached" },
            s.tabs.len(),
            s.cwd
        );
    }
    Ok(())
}

fn detach(name: Option<String>, then_attach: bool) -> Result<()> {
    let name = match name {
        Some(n) => n,
        None => {
            let sessions = session::list_sessions()?;
            match sessions.len() {
                0 => bail!("no sessions"),
                1 => sessions[0].name.clone(),
                _ => bail!(
                    "several sessions; name one: {}",
                    sessions
                        .iter()
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        }
    };
    let info = session::probe(&name)
        .with_context(|| format!("no session called {name}"))?;
    if !info.attached && !then_attach {
        bail!("session {name} is not attached");
    }
    session::detach_session(&name)?;
    if then_attach {
        client::attach(&name)
    } else {
        println!("[detached from {name}]");
        Ok(())
    }
}
