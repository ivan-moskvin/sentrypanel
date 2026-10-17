//! The pid file and the signals of `run`.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Raised by a signal handler; the loop reads it and leaves, so that the screen
/// gets its clock face back on the way out. A handler may touch nothing else.
static STOPPING: AtomicBool = AtomicBool::new(false);

pub fn stopping() -> bool {
    STOPPING.load(Ordering::Relaxed)
}

/// Asks the system to raise the flag instead of killing us outright.
pub fn catch_signals() {
    unsafe extern "C" fn raise_flag(_signal: i32) {
        STOPPING.store(true, Ordering::Relaxed);
    }
    let handler = raise_flag as *const () as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
}

/// A second instance would interleave its frames with the first one on the
/// same screen, so it refuses to start.
pub fn take_lock(path: &Path) -> Result<(), String> {
    if let Some(pid) = running(path) {
        return Err(format!("sentrypanel is already running (pid {pid})"));
    }
    std::fs::write(path, format!("{}\n", std::process::id()))
        .map_err(|err| format!("could not write {}: {err}", path.display()))
}

/// Whether the pid file still points at us. One that was removed or taken over
/// means somebody wants this instance gone.
pub fn owns_lock(path: &Path) -> bool {
    recorded_pid(path) == Some(std::process::id())
}

pub fn drop_lock(path: &Path) {
    if owns_lock(path) {
        let _ = std::fs::remove_file(path);
    }
}

pub fn running(path: &Path) -> Option<u32> {
    recorded_pid(path).filter(|pid| alive(*pid))
}

/// Asks the running instance to leave and waits until it has given the screen
/// back.
pub fn stop(path: &Path) -> bool {
    let Some(pid) = running(path) else {
        return false;
    };
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    for _ in 0..100 {
        if !alive(pid) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

fn recorded_pid(path: &Path) -> Option<u32> {
    let data = std::fs::read_to_string(path).ok()?;
    data.trim().parse::<u32>().ok().filter(|pid| *pid > 0)
}

fn alive(pid: u32) -> bool {
    // Signal zero delivers nothing and only asks whether the process is there.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("sentrypanel-{name}-{}.pid", std::process::id()))
    }

    fn exited_pid() -> u32 {
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn takes_owns_and_drops_the_lock() {
        let path = lock_path("lock");
        let _ = std::fs::remove_file(&path);

        take_lock(&path).unwrap();
        assert!(owns_lock(&path));
        assert_eq!(running(&path), Some(std::process::id()));

        let err = take_lock(&path).unwrap_err();
        assert!(err.contains("already running"), "{err}");

        drop_lock(&path);
        assert!(!path.exists());
        assert!(!owns_lock(&path));
    }

    #[test]
    fn takes_over_the_lock_of_a_process_that_is_gone() {
        let path = lock_path("stale");
        std::fs::write(&path, format!("{}\n", exited_pid())).unwrap();

        assert_eq!(running(&path), None);
        assert!(!stop(&path));
        take_lock(&path).unwrap();
        assert!(owns_lock(&path));

        drop_lock(&path);
    }

    #[test]
    fn leaves_a_lock_that_is_not_ours() {
        let path = lock_path("foreign");
        std::fs::write(&path, "1\n").unwrap();

        assert!(!owns_lock(&path));
        drop_lock(&path);
        assert!(path.exists());

        std::fs::write(&path, "garbage").unwrap();
        assert_eq!(running(&path), None);
        std::fs::remove_file(&path).unwrap();
    }
}
