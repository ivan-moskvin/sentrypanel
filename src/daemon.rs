//! The pid file of `run`, and how it is told to stop.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Raised when the user or the system asks us to leave; the loop reads it and
/// leaves, so that the screen gets its clock face back on the way out. A signal
/// handler may touch nothing else.
static STOPPING: AtomicBool = AtomicBool::new(false);

/// Raised once the bridge is dropped and the screen is given back.
#[cfg_attr(unix, allow(dead_code))]
static FINISHED: AtomicBool = AtomicBool::new(false);

/// How long a stopped instance is given to restore the screen before it is
/// ended outright.
const GRACE: Duration = Duration::from_secs(15);

pub fn stopping() -> bool {
    STOPPING.load(Ordering::Relaxed)
}

pub fn finished() {
    FINISHED.store(true, Ordering::Relaxed);
}

/// Asks the system to raise the flag instead of killing us outright.
#[cfg(unix)]
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

/// Windows has no SIGTERM: Ctrl+C, a closed console, a logoff and a shutdown
/// arrive as console events instead.
#[cfg(windows)]
pub fn catch_signals() {
    use windows_sys::Win32::System::Console::{CTRL_CLOSE_EVENT, SetConsoleCtrlHandler};

    unsafe extern "system" fn raise_flag(event: u32) -> i32 {
        STOPPING.store(true, Ordering::Relaxed);
        // After a close, a logoff or a shutdown the process is ended as soon as
        // the handler returns: holding it gives the loop the moment it needs to
        // put the clock face back.
        if event >= CTRL_CLOSE_EVENT {
            for _ in 0..40 {
                if FINISHED.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        1
    }
    unsafe {
        SetConsoleCtrlHandler(Some(raise_flag), 1);
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
/// back. Answers whether there was one.
pub fn stop(path: &Path) -> bool {
    let Some(pid) = running(path) else {
        return false;
    };
    ask_to_leave(pid, path);
    let step = Duration::from_millis(100);
    let mut waited = Duration::ZERO;
    while alive(pid) && waited < GRACE {
        std::thread::sleep(step);
        waited += step;
    }
    if alive(pid) {
        force_kill(pid);
        let _ = std::fs::remove_file(path);
    }
    true
}

fn recorded_pid(path: &Path) -> Option<u32> {
    let data = std::fs::read_to_string(path).ok()?;
    data.trim().parse::<u32>().ok().filter(|pid| *pid > 0)
}

#[cfg(unix)]
fn ask_to_leave(pid: u32, _lock: &Path) {
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
}

/// A process without a console of its own cannot be sent a console event, so
/// the pid file is taken away: the loop sees it gone and leaves as it would on
/// a signal.
#[cfg(windows)]
fn ask_to_leave(_pid: u32, lock: &Path) {
    let _ = std::fs::remove_file(lock);
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // Signal zero delivers nothing and only asks whether the process is there.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(unix)]
fn force_kill(pid: u32) {
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let asked = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        asked != 0 && code == STILL_ACTIVE as u32
    }
}

#[cfg(windows)]
fn force_kill(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};

    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return;
        }
        TerminateProcess(handle, 1);
        CloseHandle(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("sentrypanel-{name}-{}.pid", std::process::id()))
    }

    /// The pid of a process that has already exited. The shell that ends right
    /// away is spelled differently on every system.
    fn exited_pid() -> u32 {
        let mut command = if cfg!(windows) {
            let mut command = std::process::Command::new("cmd");
            command.args(["/C", "exit"]);
            command
        } else {
            let mut command = std::process::Command::new("/bin/sh");
            command.args(["-c", "exit 0"]);
            command
        };
        let mut child = command.spawn().unwrap();
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

    #[test]
    fn knows_a_process_that_is_there_from_one_that_is_not() {
        assert!(alive(std::process::id()), "we are running");
        assert!(!alive(exited_pid()));
    }
}
