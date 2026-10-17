//! `service install|uninstall`: `run` as a per-user service — a launchd agent
//! on macOS, a systemd user unit on Linux, a program started at login on
//! Windows.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;
use crate::{daemon, device, paths, run};

const LABEL: &str = "io.github.ivan-moskvin.sentrypanel";
const UNIT: &str = "sentrypanel.service";

/// The per-user list of programs started at login. Task Scheduler would want
/// administrator rights for a logon trigger; this key does not.
#[cfg(any(windows, test))]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(any(windows, test))]
const RUN_VALUE: &str = "sentrypanel";

pub fn run(action: &str, config: &Path) -> Result<(), String> {
    if !matches!(action, "install" | "uninstall") {
        return Err(format!(
            "unknown service action {action:?}: install or uninstall"
        ));
    }
    let exe = crate::update::self_path()?;
    let home = dirs::home_dir().ok_or("no home directory")?;
    // The service starts in another directory with another environment: a
    // relative path would point somewhere else there.
    let config = std::path::absolute(config).map_err(|err| err.to_string())?;

    match action {
        "install" => {
            prepare(&config)?;
            install(&exe, &config, &home)
        }
        _ => uninstall(&home),
    }
}

/// A service with a broken config would only restart over and over, so the
/// config is checked first. The device is looked for here, in the terminal,
/// and its address remembered: a process the system starts at login may not
/// see the local network yet.
fn prepare(config_path: &Path) -> Result<(), String> {
    let config = Config::load(config_path)?;
    if config.device.ip.is_some() {
        return Ok(());
    }
    match device::locate(&config.device, &paths::file(run::KNOWN)?) {
        Ok(located) if !located.remembered => {
            println!("Found the device at {}", located.device.ip())
        }
        Ok(located) => println!(
            "No device seen on the network now; the panel starts with the address it was last found at, {}",
            located.device.ip()
        ),
        Err(err) => println!("{err}; the panel keeps looking once it runs"),
    }
    Ok(())
}

/// Stops the running panel and has it started again, so that it picks up a
/// new screen or a new binary.
pub fn restart(lock: &Path, config: &Path) -> Result<(), String> {
    if !daemon::stop(lock) {
        return Ok(());
    }
    if !installed() {
        println!("The panel was stopped; start it again: sentrypanel run");
        return Ok(());
    }
    start_again(config)
}

/// launchd and systemd bring the service back by themselves.
#[cfg(unix)]
fn start_again(_config: &Path) -> Result<(), String> {
    println!("The service starts the panel again");
    Ok(())
}

/// Nothing watches a program started at login, so it is started here.
#[cfg(windows)]
fn start_again(config: &Path) -> Result<(), String> {
    let config = std::path::absolute(config).map_err(|err| err.to_string())?;
    start_now(&crate::update::self_path()?, &config, &paths::log_file()?)?;
    println!("The panel is started again");
    Ok(())
}

#[cfg(target_os = "macos")]
fn install(exe: &Path, config: &Path, home: &Path) -> Result<(), String> {
    install_launchd(exe, config, home)
}

#[cfg(target_os = "macos")]
fn uninstall(home: &Path) -> Result<(), String> {
    uninstall_launchd(home)
}

#[cfg(target_os = "linux")]
fn install(exe: &Path, config: &Path, home: &Path) -> Result<(), String> {
    install_systemd(exe, config, home)
}

#[cfg(target_os = "linux")]
fn uninstall(home: &Path) -> Result<(), String> {
    uninstall_systemd(home)
}

#[cfg(windows)]
fn install(exe: &Path, config: &Path, _home: &Path) -> Result<(), String> {
    install_windows(exe, config)
}

#[cfg(windows)]
fn uninstall(_home: &Path) -> Result<(), String> {
    uninstall_windows()
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn install(_exe: &Path, _config: &Path, _home: &Path) -> Result<(), String> {
    Err(format!("no service support on {}", std::env::consts::OS))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn uninstall(_home: &Path) -> Result<(), String> {
    Err(format!("no service support on {}", std::env::consts::OS))
}

#[cfg(unix)]
pub fn installed() -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    plist_path(&home).exists() || unit_path(&home).exists()
}

#[cfg(windows)]
pub fn installed() -> bool {
    Command::new("reg")
        .args(["query", RUN_KEY, "/v", RUN_VALUE])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn plist_path(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn unit_path(home: &Path) -> PathBuf {
    home.join(".config/systemd/user").join(UNIT)
}

#[cfg(target_os = "macos")]
fn install_launchd(exe: &Path, config: &Path, home: &Path) -> Result<(), String> {
    let path = plist_path(home);
    let log = home.join("Library/Logs/sentrypanel.log");
    write(&path, &plist(exe, config, &log))?;

    let domain = format!("gui/{}", unsafe { libc::getuid() });
    // An agent loaded from an older plist keeps running the old one.
    let _ = Command::new("launchctl")
        .args(["bootout", &format!("{domain}/{LABEL}")])
        .output();
    command(
        "launchctl",
        &["bootstrap", &domain, &path.to_string_lossy()],
    )?;

    println!("Installed {}", path.display());
    println!("Log: {}", log.display());
    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall_launchd(home: &Path) -> Result<(), String> {
    let path = plist_path(home);
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let _ = Command::new("launchctl")
        .args(["bootout", &format!("{domain}/{LABEL}")])
        .output();
    remove(&path)
}

#[cfg(target_os = "linux")]
fn install_systemd(exe: &Path, config: &Path, home: &Path) -> Result<(), String> {
    let path = unit_path(home);
    write(&path, &unit(exe, config))?;
    command("systemctl", &["--user", "daemon-reload"])?;
    command("systemctl", &["--user", "enable", "--now", UNIT])?;
    command("systemctl", &["--user", "restart", UNIT])?;

    println!("Installed {}", path.display());
    println!("Log: journalctl --user -u {UNIT}");
    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_systemd(home: &Path) -> Result<(), String> {
    let _ = Command::new("systemctl")
        .args(["--user", "disable", "--now", UNIT])
        .output();
    remove(&unit_path(home))?;
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    Ok(())
}

#[cfg(windows)]
fn install_windows(exe: &Path, config: &Path) -> Result<(), String> {
    let log = paths::log_file()?;
    let custom = (paths::config()? != config).then_some(config);
    let line = login_command(exe, custom, &log)?;
    command(
        "reg",
        &[
            "add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", &line, "/f",
        ],
    )?;

    // The panel already running holds the old binary or the old config.
    daemon::stop(&paths::file(run::LOCK)?);
    start_now(exe, config, &log)?;

    println!("Installed: starts at login ({RUN_KEY}\\{RUN_VALUE})");
    println!("Log: {}", log.display());
    Ok(())
}

#[cfg(windows)]
fn uninstall_windows() -> Result<(), String> {
    match command("reg", &["delete", RUN_KEY, "/v", RUN_VALUE, "/f"]) {
        Ok(()) => println!("Removed {RUN_KEY}\\{RUN_VALUE}"),
        Err(_) => println!("No service was installed"),
    }
    if daemon::stop(&paths::file(run::LOCK)?) {
        println!("The panel is stopped");
    }
    Ok(())
}

/// The panel is a console program, and Windows opens a window for a console
/// program started at login. A headless console host gives it a console that
/// is never shown — and does so before the program runs, so not even a flash
/// of a window is seen. A program that hid its own window would show it first.
///
/// Windows skips an entry of the key longer than 260 characters without a
/// word, so the default config is not spelled out and a longer line is refused
/// here.
#[cfg(any(windows, test))]
fn login_command(exe: &Path, config: Option<&Path>, log: &Path) -> Result<String, String> {
    let mut line = format!("conhost.exe --headless \"{}\" run", exe.display());
    if let Some(config) = config {
        line.push_str(&format!(" --config \"{}\"", config.display()));
    }
    line.push_str(&format!(" --log \"{}\"", log.display()));
    match line.chars().count() {
        0..=260 => Ok(line),
        length => Err(format!(
            "the command that starts the panel at login is {length} characters long, \
             and Windows runs none longer than 260 — move the binary or the config to a shorter path"
        )),
    }
}

/// Started from a terminal, the panel needs no console host: it is given a
/// console of its own that is never shown, and Ctrl+C in the terminal does not
/// reach it.
#[cfg(windows)]
fn start_now(exe: &Path, config: &Path, log: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};

    Command::new(exe)
        .arg("run")
        .arg("--config")
        .arg(config)
        .arg("--log")
        .arg(log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not start the panel: {err}"))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn plist(exe: &Path, config: &Path, log: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>run</string>
    <string>--config</string>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ThrottleInterval</key>
  <integer>30</integer>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
        xml(&exe.to_string_lossy()),
        xml(&config.to_string_lossy()),
        xml(&log.to_string_lossy()),
        xml(&log.to_string_lossy()),
    )
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn unit(exe: &Path, config: &Path) -> String {
    format!(
        "[Unit]\n\
         Description=Sentry numbers on a Divoom Times Gate\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         ExecStart=\"{}\" run --config \"{}\"\n\
         Restart=always\n\
         RestartSec=30\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe.display(),
        config.display()
    )
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg_attr(windows, allow(dead_code))]
fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|err| format!("could not write {}: {err}", path.display()))
}

#[cfg_attr(windows, allow(dead_code))]
fn remove(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => {
            println!("Removed {}", path.display());
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!("No service was installed");
            Ok(())
        }
        Err(err) => Err(format!("could not remove {}: {err}", path.display())),
    }
}

fn command(program: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| format!("could not run {program}: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{program} {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_runs_the_panel_with_its_config() {
        let text = plist(
            Path::new("/usr/local/bin/sentrypanel"),
            Path::new("/Users/me/Library/Application Support/sentrypanel/a&b.toml"),
            Path::new("/Users/me/Library/Logs/sentrypanel.log"),
        );

        assert!(text.contains(&format!("<string>{LABEL}</string>")));
        assert!(
            text.contains("<string>/usr/local/bin/sentrypanel</string>\n    <string>run</string>")
        );
        assert!(text.contains("Application Support/sentrypanel/a&amp;b.toml"));
        assert!(text.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(text.contains("<key>KeepAlive</key>\n  <true/>"));
    }

    #[test]
    fn the_unit_runs_the_panel_with_its_config() {
        let text = unit(
            Path::new("/home/me/.cargo/bin/sentrypanel"),
            Path::new("/home/me/.config/sentrypanel/config.toml"),
        );
        assert!(text.contains(
            "ExecStart=\"/home/me/.cargo/bin/sentrypanel\" run --config \"/home/me/.config/sentrypanel/config.toml\""
        ));
        assert!(text.contains("Restart=always"));
        assert!(text.contains("WantedBy=default.target"));
    }

    #[test]
    fn knows_where_each_system_keeps_them() {
        let home = Path::new("/home/me");
        assert_eq!(
            plist_path(home),
            PathBuf::from(format!("/home/me/Library/LaunchAgents/{LABEL}.plist"))
        );
        assert_eq!(
            unit_path(home),
            PathBuf::from("/home/me/.config/systemd/user/sentrypanel.service")
        );
    }

    #[test]
    fn refuses_an_unknown_action() {
        let err = run("restart", Path::new("config.toml")).unwrap_err();
        assert!(err.contains("restart"), "{err}");
    }

    #[test]
    fn writes_and_removes_a_file() {
        let dir = std::env::temp_dir().join(format!("sentrypanel-service-{}", std::process::id()));
        let path = dir.join("nested/unit.service");

        write(&path, "x").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x");
        remove(&path).unwrap();
        remove(&path).unwrap();
        assert!(!path.exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn starts_the_panel_at_login_in_a_hidden_console() {
        let exe = Path::new(r"C:\Users\me\AppData\Local\sentrypanel\sentrypanel.exe");
        let log = Path::new(r"C:\Users\me\AppData\Local\sentrypanel\sentrypanel.log");
        assert_eq!(
            login_command(exe, None, log).unwrap(),
            r#"conhost.exe --headless "C:\Users\me\AppData\Local\sentrypanel\sentrypanel.exe" run --log "C:\Users\me\AppData\Local\sentrypanel\sentrypanel.log""#
        );

        let custom = login_command(exe, Some(Path::new(r"D:\panel.toml")), log).unwrap();
        assert!(
            custom.contains(r#" run --config "D:\panel.toml" --log "#),
            "{custom}"
        );

        let deep = PathBuf::from("D:\\").join("very-long-folder-name".repeat(12));
        let err = login_command(exe, Some(&deep), log).unwrap_err();
        assert!(err.contains("260"), "{err}");

        assert!(
            RUN_KEY.starts_with(r"HKCU\"),
            "no administrator rights needed"
        );
        assert_eq!(RUN_VALUE, "sentrypanel");
    }

    #[cfg(unix)]
    #[test]
    fn reports_a_failing_command() {
        assert!(command("/bin/sh", &["-c", "exit 0"]).is_ok());
        let err = command("/bin/sh", &["-c", "echo nope >&2; exit 3"]).unwrap_err();
        assert!(err.contains("nope"), "{err}");
        assert!(command("/definitely/not/a/program", &[]).is_err());
    }
}
