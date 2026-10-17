//! Key Sentry numbers on a screen of a Divoom Times Gate.

#[cfg(not(unix))]
compile_error!("sentrypanel runs on macOS and Linux");

mod args;
mod commands;
mod config;
mod daemon;
mod demo;
mod device;
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

    if let Err(err) = dispatch(args) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn dispatch(args: args::Args) -> Result<(), String> {
    let config_path = match args.config {
        Some(path) => path,
        None => paths::config()?,
    };

    match args.command {
        Command::Run => run::run(&config_path),
        Command::Once => commands::once(&config_path),
        Command::Preview { file, demo } => commands::preview(&config_path, &file, demo),
        Command::Devices => commands::devices(),
        Command::Screen(asked) => {
            commands::screen(&config_path, asked.as_deref(), &paths::file(run::LOCK)?)
        }
        Command::Service(action) => service::run(&action, &config_path),
        Command::Version => {
            println!("sentrypanel {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::Help => {
            print!("{USAGE}");
            Ok(())
        }
    }
}
