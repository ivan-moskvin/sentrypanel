//! `service install|uninstall`: `run` as a per-user service — a launchd agent
//! on macOS, a systemd user unit on Linux.

use std::path::{Path, PathBuf};
use std::process::Command;

const LABEL: &str = "io.github.ivan-moskvin.sentrypanel";
const UNIT: &str = "sentrypanel.service";

pub fn run(action: &str, config: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|err| format!("could not find ourselves: {err}"))?;
    let home = dirs::home_dir().ok_or("no home directory")?;
    // The service starts in another directory with another environment: a
    // relative path would point somewhere else there.
    let config = std::path::absolute(config).map_err(|err| err.to_string())?;

    match (action, std::env::consts::OS) {
        ("install", "macos") => install_launchd(&exe, &config, &home),
        ("uninstall", "macos") => uninstall_launchd(&home),
        ("install", "linux") => install_systemd(&exe, &config, &home),
        ("uninstall", "linux") => uninstall_systemd(&home),
        ("install" | "uninstall", os) => Err(format!("no service support on {os}")),
        (other, _) => Err(format!(
            "unknown service action {other:?}: install or uninstall"
        )),
    }
}

pub fn installed() -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    plist_path(&home).exists() || unit_path(&home).exists()
}

fn plist_path(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

fn unit_path(home: &Path) -> PathBuf {
    home.join(".config/systemd/user").join(UNIT)
}

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

fn uninstall_launchd(home: &Path) -> Result<(), String> {
    let path = plist_path(home);
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let _ = Command::new("launchctl")
        .args(["bootout", &format!("{domain}/{LABEL}")])
        .output();
    remove(&path)
}

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

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|err| format!("could not write {}: {err}", path.display()))
}

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
    fn reports_a_failing_command() {
        assert!(command("/bin/sh", &["-c", "exit 0"]).is_ok());
        let err = command("/bin/sh", &["-c", "echo nope >&2; exit 3"]).unwrap_err();
        assert!(err.contains("nope"), "{err}");
        assert!(command("/definitely/not/a/program", &[]).is_err());
    }
}
