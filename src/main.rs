//! Key Sentry numbers on a screen of a Divoom Times Gate.

mod args;
mod commands;
mod config;
mod daemon;
mod demo;
mod device;
mod log;
mod metric;
mod paths;
mod render;
mod run;
mod screen;
mod sentry;
mod service;
mod state;
mod table;
#[cfg(test)]
mod testserver;
mod token;
mod uninstall;
mod update;
mod view;

use args::{Command, USAGE};

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match args::parse(&raw) {
        Ok(args) => args,
        Err(err) => {
            eprint!("{err}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    // A panel started at login on Windows has no console: what it fails with
    // has to reach its log, or nobody learns why the screen stayed dark.
    if let Some(log) = &args.log {
        log::to_file(log);
    }
    let logged = args.log.is_some();

    if let Err(err) = dispatch(args) {
        if logged {
            log::line(&err);
        }
        eprintln!("{err}");
        std::process::exit(1);
    }
}

/// The version is stamped into a release build by CI. A hand-made build has
/// none: it counts as outdated, and `update` replaces it with a release.
pub fn version() -> String {
    match option_env!("SENTRYPANEL_VERSION") {
        Some(tag) if !tag.is_empty() => tag.to_string(),
        _ => "built from source".to_string(),
    }
}

fn dispatch(args: args::Args) -> Result<(), String> {
    let config_path = match args.config {
        Some(path) => path,
        None => paths::config()?,
    };

    match args.command {
        Command::Run => run::run(&config_path),
        Command::Stop => commands::stop(&paths::file(run::LOCK)?),
        Command::Once => commands::once(&config_path),
        Command::Preview { file, demo } => commands::preview(&config_path, &file, demo),
        Command::Devices => commands::devices(),
        Command::Screen(asked) => {
            commands::screen(&config_path, asked.as_deref(), &paths::file(run::LOCK)?)
        }
        Command::Service(action) => service::run(&action, &config_path),
        Command::Config { init } => commands::config(&config_path, init),
        Command::Check => update::check(),
        Command::Update => update::update(&config_path),
        Command::Uninstall { purge } => uninstall::uninstall(&config_path, purge),
        Command::Version => {
            println!("sentrypanel {}", version());
            Ok(())
        }
        Command::Help => {
            print!("{USAGE}");
            Ok(())
        }
    }
}
