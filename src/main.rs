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
            // `scr -S name` is "ensure this session" — an existing daemon is
            // reused (and taken over on attach) rather than being an error.
            if session::probe(&name).is_some() {
                if !command.is_empty() {
                    bail!(
                        "session {name} already exists; use scr -r {name}, or pick another name"
                    );
                }
                return client::attach(&name);
            }
            session::spawn_server(&name, &command)?;
            client::attach(&name)
        }
        Command::Resume { name } => resume(name),
        Command::List => list(),
    }
}

fn resume(name: Option<String>) -> Result<()> {
    let sessions = session::list_sessions()?;
    if let Some(name) = name {
        // Verify the session exists so a typo gets a clear message; attaching
        // itself is unconditional and takes over from any other client.
        session::probe(&name).with_context(|| format!("no session called {name}"))?;
        return client::attach(&name);
    }
    match sessions.len() {
        0 => bail!("no sessions to resume"),
        1 => client::attach(&sessions[0].name),
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
