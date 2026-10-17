//! The command line.

use std::path::PathBuf;

pub const USAGE: &str = "sentrypanel — key Sentry numbers on a screen of a Divoom Times Gate.

Usage:
  sentrypanel run [--log FILE]        keep the screen updated
  sentrypanel stop                    stop the running panel
  sentrypanel once                    fetch once and print the numbers
  sentrypanel preview [--demo] FILE   draw the panel into a GIF file
  sentrypanel devices                 list the Divoom devices on the network
  sentrypanel screen [N]              show or change the screen, 1–5
  sentrypanel config                  print the path of the config
  sentrypanel config init             create the config from the example
  sentrypanel service install         start the panel at login, now and on
  sentrypanel service uninstall       remove the service
  sentrypanel check                   check whether a new version is out
  sentrypanel update                  install the latest version
  sentrypanel uninstall [--purge]     remove the service and the binary;
                                      --purge removes the config too
  sentrypanel version                 print the version

Options:
  --config PATH   the config file instead of the default one
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub config: Option<PathBuf>,
    pub log: Option<PathBuf>,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run,
    Stop,
    Once,
    Preview { file: PathBuf, demo: bool },
    Devices,
    Screen(Option<String>),
    Service(String),
    Config { init: bool },
    Check,
    Update,
    Uninstall { purge: bool },
    Version,
    Help,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut config = None;
    let mut log = None;
    let mut words = Vec::new();
    let mut demo = false;
    let mut purge = false;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" | "-c" => {
                let path = iter.next().ok_or("--config needs a path")?;
                config = Some(PathBuf::from(path));
            }
            "--log" => {
                let path = iter.next().ok_or("--log needs a path")?;
                log = Some(PathBuf::from(path));
            }
            "--demo" => demo = true,
            "--purge" => purge = true,
            "--help" | "-h" => words.insert(0, "help".to_string()),
            "--version" | "-V" => words.insert(0, "version".to_string()),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            word => words.push(word.to_string()),
        }
    }

    let command = command(&words, demo, purge)?;
    if demo && !matches!(command, Command::Preview { .. }) {
        return Err("--demo is an option of preview".into());
    }
    if purge && !matches!(command, Command::Uninstall { .. }) {
        return Err("--purge is an option of uninstall".into());
    }
    if log.is_some() && command != Command::Run {
        return Err("--log is an option of run".into());
    }
    Ok(Args {
        config,
        log,
        command,
    })
}

fn command(words: &[String], demo: bool, purge: bool) -> Result<Command, String> {
    let rest = |count: usize| -> Result<(), String> {
        match words.get(count) {
            Some(extra) => Err(format!("unexpected argument {extra:?}")),
            None => Ok(()),
        }
    };

    let command = match words.first().map(String::as_str) {
        None | Some("help") => Command::Help,
        Some("run") => Command::Run,
        Some("stop") => Command::Stop,
        Some("check") => Command::Check,
        Some("update") => Command::Update,
        Some("uninstall") => Command::Uninstall { purge },
        Some("once") => Command::Once,
        Some("devices") => Command::Devices,
        Some("version") => Command::Version,
        Some("screen") => {
            rest(2)?;
            return Ok(Command::Screen(words.get(1).cloned()));
        }
        Some("preview") => {
            rest(2)?;
            let file = words
                .get(1)
                .ok_or("name a file: sentrypanel preview panel.gif")?;
            return Ok(Command::Preview {
                file: PathBuf::from(file),
                demo,
            });
        }
        Some("config") => {
            rest(2)?;
            return match words.get(1).map(String::as_str) {
                None => Ok(Command::Config { init: false }),
                Some("init") => Ok(Command::Config { init: true }),
                Some(other) => Err(format!(
                    "unknown config action {other:?}: config or config init"
                )),
            };
        }
        Some("service") => {
            rest(2)?;
            let action = words.get(1).ok_or("service install or service uninstall")?;
            return Ok(Command::Service(action.clone()));
        }
        Some(other) => return Err(format!("unknown command {other:?}")),
    };
    rest(1)?;
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(line: &str) -> Result<Args, String> {
        let args: Vec<String> = line.split_whitespace().map(String::from).collect();
        parse(&args)
    }

    fn command_of(line: &str) -> Command {
        parsed(line).unwrap().command
    }

    #[test]
    fn reads_every_command() {
        assert_eq!(command_of(""), Command::Help);
        assert_eq!(command_of("--help"), Command::Help);
        assert_eq!(command_of("run"), Command::Run);
        assert_eq!(command_of("stop"), Command::Stop);
        assert_eq!(command_of("check"), Command::Check);
        assert_eq!(command_of("update"), Command::Update);
        assert_eq!(command_of("uninstall"), Command::Uninstall { purge: false });
        assert_eq!(
            command_of("uninstall --purge"),
            Command::Uninstall { purge: true }
        );
        assert_eq!(command_of("config"), Command::Config { init: false });
        assert_eq!(command_of("config init"), Command::Config { init: true });
        assert_eq!(command_of("once"), Command::Once);
        assert_eq!(command_of("devices"), Command::Devices);
        assert_eq!(command_of("version"), Command::Version);
        assert_eq!(command_of("--version"), Command::Version);
        assert_eq!(command_of("screen"), Command::Screen(None));
        assert_eq!(command_of("screen 3"), Command::Screen(Some("3".into())));
        assert_eq!(
            command_of("service install"),
            Command::Service("install".into())
        );
        assert_eq!(
            command_of("preview --demo out.gif"),
            Command::Preview {
                file: "out.gif".into(),
                demo: true
            }
        );
    }

    #[test]
    fn takes_the_config_anywhere() {
        let args = parsed("once --config /tmp/a.toml").unwrap();
        assert_eq!(args.config, Some(PathBuf::from("/tmp/a.toml")));
        assert_eq!(
            parsed("-c b.toml run").unwrap().config,
            Some("b.toml".into())
        );
        assert!(parsed("run --config").is_err());
    }

    #[test]
    fn takes_a_log_file_for_run_only() {
        let args = parsed("run --log /tmp/panel.log").unwrap();
        assert_eq!(args.log, Some(PathBuf::from("/tmp/panel.log")));
        assert!(parsed("once --log a.log").is_err());
        assert!(parsed("run --log").is_err());
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        for line in [
            "fly",
            "run now",
            "preview",
            "preview a.gif b.gif",
            "service",
            "screen 1 2",
            "run --demo",
            "run --verbose",
            "stop --purge",
            "config edit",
            "config init now",
            "update now",
        ] {
            assert!(parsed(line).is_err(), "{line}");
        }
    }
}
