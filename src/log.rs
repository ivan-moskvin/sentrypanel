//! The lines `run` writes as it goes. They go to stderr, which launchd and
//! systemd keep; `run --log FILE` sends them to a file instead, for the
//! Windows panel that runs with no console.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static FILE: OnceLock<PathBuf> = OnceLock::new();

/// A log nobody reads would otherwise grow for as long as the machine lives:
/// past this size it starts over, the previous one kept as `.old`.
const LIMIT: u64 = 1024 * 1024;

pub fn to_file(path: &Path) {
    rotate(path, LIMIT);
    let _ = FILE.set(path.to_path_buf());
}

pub fn line(message: &str) {
    let text = format!(
        "{} {message}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
    match FILE.get() {
        Some(path) => append(path, &text),
        None => eprint!("{text}"),
    }
}

fn append(path: &Path, text: &str) {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path);
    match file {
        Ok(mut file) => {
            let _ = file.write_all(text.as_bytes());
        }
        Err(_) => eprint!("{text}"),
    }
}

fn rotate(path: &Path, limit: u64) {
    let too_big = std::fs::metadata(path).is_ok_and(|meta| meta.len() > limit);
    if too_big {
        let mut old = path.as_os_str().to_os_string();
        old.push(".old");
        let _ = std::fs::rename(path, old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_and_starts_over_past_the_limit() {
        let dir = std::env::temp_dir().join(format!("sentrypanel-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("panel.log");

        append(&path, "one\n");
        append(&path, "two\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");

        rotate(&path, 100);
        assert!(path.exists(), "a small log is left alone");
        rotate(&path, 4);
        assert!(!path.exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("panel.log.old")).unwrap(),
            "one\ntwo\n"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
