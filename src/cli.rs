//! Command line parsing. `scr` follows the GNU Screen spellings where they
//! still make sense for a tabbed multiplexer.

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Internal daemon mode: `scr --server <name> [command…]`.
    Server { name: String, command: Vec<String> },
    New { name: Option<String>, command: Vec<String> },
    Resume { name: Option<String> },
    List,
    Detach { name: Option<String>, then_attach: bool },
    Help,
    Version,
}

pub const USAGE: &str = "\
scr — a lightweight, detachable GNU Screen replacement

USAGE:
    scr                       new session, then attach
    scr <command> [args…]     new session running a command
    scr -S <name>             new named session
    scr -r [name]             resume a session; no name opens the picker
    scr -ls                   list sessions
    scr -d [name]             detach a session
    scr -dr <name>            detach, then attach

KEYS:
    Ctrl+A C      new tab            Ctrl+A D      detach
    Ctrl+A \"      select tab         Ctrl+A K      close tab
    Ctrl+A N / P  next / previous    Ctrl+A 1…9    jump to tab
    Alt+← / →     next / previous    Alt+1…9       jump to tab
    Ctrl+A A      literal Ctrl+A     Ctrl+A ?      help

Mouse: click tabs, + and ×; wheel scrolls the terminal history.
";

pub fn parse(args: Vec<String>) -> Result<Command> {
    if args.first().map(String::as_str) == Some("--server") {
        let name = args
            .get(1)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("--server needs a session name"))?;
        return Ok(Command::Server {
            name,
            command: args.into_iter().skip(2).collect(),
        });
    }

    let mut resume = false;
    let mut detach = false;
    let mut list = false;
    let mut then_attach = false;
    let mut name: Option<String> = None;
    let mut command: Vec<String> = Vec::new();
    let mut stop = false;
    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        if stop {
            command.push(arg.clone());
            i += 1;
            continue;
        }
        match arg.as_str() {
            "--" => stop = true,
            "-h" | "--help" => return Ok(Command::Help),
            "-v" | "--version" => return Ok(Command::Version),
            "-ls" | "--list" | "-list" => list = true,
            "-r" | "--resume" => resume = true,
            "-d" | "--detach" => detach = true,
            "-dr" | "--detach-resume" => {
                detach = true;
                resume = true;
                then_attach = true;
            }
            "-S" | "--session-name" => {
                i += 1;
                let Some(value) = args.get(i) else {
                    bail!("{arg} needs a name");
                };
                name = Some(value.clone());
            }
            s if s.starts_with('-') => bail!("unknown option {s}; try scr --help"),
            _ => {
                if (resume || detach) && name.is_none() {
                    name = Some(arg.clone());
                } else if (resume || detach) && !command.is_empty() {
                    bail!("unexpected argument {arg}");
                } else {
                    command.push(arg.clone());
                }
            }
        }
        i += 1;
    }

    if list {
        if resume || detach || !command.is_empty() {
            bail!("-ls cannot be combined with other actions");
        }
        return Ok(Command::List);
    }
    if detach {
        if !command.is_empty() {
            bail!("-d does not take a command");
        }
        return Ok(Command::Detach { name, then_attach });
    }
    if resume {
        if !command.is_empty() {
            bail!("-r takes a session name, not a command");
        }
        return Ok(Command::Resume { name });
    }
    Ok(Command::New { name, command })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Command {
        parse(args.iter().map(|s| s.to_string()).collect()).unwrap()
    }

    #[test]
    fn plain_scr_is_a_new_session() {
        assert_eq!(
            p(&[]),
            Command::New {
                name: None,
                command: vec![]
            }
        );
    }

    #[test]
    fn a_command_becomes_the_first_window() {
        assert_eq!(
            p(&["htop"]),
            Command::New {
                name: None,
                command: vec!["htop".into()]
            }
        );
    }

    #[test]
    fn resume_can_name_a_session_or_ask_for_the_picker() {
        assert_eq!(p(&["-r"]), Command::Resume { name: None });
        assert_eq!(
            p(&["-r", "work"]),
            Command::Resume {
                name: Some("work".into())
            }
        );
    }

    #[test]
    fn named_and_detach_flags_parse() {
        assert_eq!(
            p(&["-S", "work"]),
            Command::New {
                name: Some("work".into()),
                command: vec![]
            }
        );
        assert_eq!(
            p(&["-dr", "work"]),
            Command::Detach {
                name: Some("work".into()),
                then_attach: true
            }
        );
    }

    #[test]
    fn unknown_options_are_refused() {
        assert!(parse(vec!["-z".into()]).is_err());
    }
}
