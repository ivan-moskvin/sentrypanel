//! `uninstall`: the service, the running panel, our state and the binary go;
//! the config stays unless `--purge` is given — it is the one file that took
//! work to write, and the same file serves the next machine.

use std::path::Path;

use crate::{daemon, paths, run, service, update};

pub fn uninstall(config: &Path, purge: bool) -> Result<(), String> {
    let exe = update::self_path()?;

    service::run("uninstall", config)?;
    if daemon::stop(&paths::file(run::LOCK)?) {
        println!("The panel is stopped");
    }

    let dir = paths::dir()?;
    match purge {
        true => remove_dir(&dir),
        false => {
            for name in [run::LOCK, run::FACE, run::KNOWN] {
                let _ = std::fs::remove_file(dir.join(name));
            }
            if config.exists() {
                println!(
                    "The config is kept: {} (sentrypanel uninstall --purge removes it)",
                    config.display()
                );
            }
        }
    }
    remove_log();
    remove_binary(&exe);
    Ok(())
}

fn remove_dir(dir: &Path) {
    if std::fs::remove_dir_all(dir).is_ok() {
        println!("Removed {}", dir.display());
    }
}

#[cfg(windows)]
fn remove_log() {
    if let Ok(log) = paths::log_file() {
        let _ = std::fs::remove_file(&log);
        let _ = std::fs::remove_file(log.with_extension("log.old"));
    }
}

/// launchd and systemd keep their logs where they keep everyone else's.
#[cfg(unix)]
fn remove_log() {}

#[cfg(unix)]
fn remove_binary(exe: &Path) {
    match std::fs::remove_file(exe) {
        Ok(()) => println!("Removed the binary {}", exe.display()),
        Err(err) => println!(
            "The binary is still there — remove it by hand: {} ({err})",
            exe.display()
        ),
    }
}

/// Windows will not delete a running file, and we are it. A hidden shell does
/// it once we are gone, and takes the directory along if nothing else is left
/// in it.
#[cfg(windows)]
fn remove_binary(exe: &Path) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    let dir = exe.parent().unwrap_or(exe);
    let script = format!(
        "/C ping -n 3 127.0.0.1 >nul & del /F /Q \"{}\" & rmdir \"{}\" 2>nul",
        exe.display(),
        dir.display()
    );
    let started = std::process::Command::new("cmd")
        .raw_arg(script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    match started {
        Ok(_) => println!("The binary {} is removed in a moment", exe.display()),
        Err(_) => println!(
            "The binary is still there — remove it by hand: {}",
            exe.display()
        ),
    }
}
