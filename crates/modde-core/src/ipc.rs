//! Best-effort, fire-and-forget IPC from the CLI to running GUI(s).
//!
//! Goal: when the CLI mutates profile state (install, update, uninstall,
//! profile create/delete, import, …), every GUI process running against
//! the same data dir should refresh immediately — without polling,
//! watchers, or any CPU cost when no GUI is running.
//!
//! ## Mechanism
//!
//! Each GUI process binds a Unix domain socket at a *per-process* path
//! ([`gui_socket_path`]: `$XDG_RUNTIME_DIR/modde-${euid}-${pid}.sock`).
//! The CLI calls [`notify_refresh`], which enumerates the directory for
//! every socket matching the current user's prefix and pushes a one-line
//! payload to each. Sockets that don't accept (peer crashed without
//! unlinking, kernel returned ECONNREFUSED) are GC'd in the same pass.
//!
//! There is no protocol: the existence of any byte-stream connection is
//! the signal. Each GUI re-reads the DB on receipt.
//!
//! ## Multi-window
//!
//! Per-process sockets give us natural fan-out — N GUIs ⇒ N sockets ⇒
//! N notifications, all from one CLI call. Each GUI is responsible for
//! unlinking its own socket on shutdown via [`cleanup_socket`]; the
//! CLI's GC pass handles the case where a GUI crashed without doing so.
//!
//! ## Costs
//!
//! - GUI idle: 0. `accept().await` is a blocked syscall.
//! - GUI active, no CLI: 0.
//! - CLI op, no GUI: one `read_dir` + zero connects (no socket files).
//! - CLI op, N GUIs: one `read_dir` + N short Unix-socket round trips.

use std::io::Write as _;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

const REFRESH_PAYLOAD: &[u8] = b"refresh\n";
const SOCKET_EXTENSION: &str = "sock";
/// Connect/write timeout for the CLI side. Kept short so a stale
/// socket file (GUI crashed without unlinking) can't hang the CLI.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(50);

/// Directory we drop sockets into. `$XDG_RUNTIME_DIR` when present (the
/// systemd-managed per-user tmpfs, cleaned at logout); falls back to
/// `/tmp`. Callers should not assume the directory is private.
#[must_use]
pub fn socket_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn euid() -> u32 {
    unsafe { libc::geteuid() }
}

/// Per-user prefix used to filter sockets owned by other users on
/// shared hosts.
fn socket_prefix() -> String {
    format!("modde-{}-", euid())
}

/// Path each GUI process binds. Includes the pid so multiple GUIs run
/// side-by-side without colliding.
#[must_use]
pub fn gui_socket_path() -> PathBuf {
    let pid = std::process::id();
    socket_dir().join(format!("{}{pid}.{SOCKET_EXTENSION}", socket_prefix()))
}

/// Best-effort cleanup: unlink the socket file the current process
/// bound at startup. Safe to call from drop / signal handlers / atexit.
pub fn cleanup_socket(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Notify every running GUI (if any) that profile state has changed.
///
/// Returns the number of listeners that accepted the notification —
/// `0` is the normal case when no GUI is running. Errors are
/// swallowed; this function is safe to call from any CLI exit path.
///
/// As a side-effect, sockets that fail to connect with ECONNREFUSED or
/// ENOENT are unlinked: this keeps `$XDG_RUNTIME_DIR` from accumulating
/// dead socket files when GUIs crash.
pub fn notify_refresh() -> usize {
    notify_refresh_in(&socket_dir())
}

/// [`notify_refresh`] scoped to an explicit directory. Useful for
/// tests that don't want to mutate `XDG_RUNTIME_DIR`, and for
/// instance-isolated CLI invocations that pass a custom data dir.
pub fn notify_refresh_in(dir: &Path) -> usize {
    let prefix = socket_prefix();
    let suffix = format!(".{SOCKET_EXTENSION}");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };

    let mut delivered = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.starts_with(&prefix) || !name_str.ends_with(&suffix) {
            continue;
        }
        let path = entry.path();
        if notify_refresh_at(&path) {
            delivered += 1;
        } else {
            // Connect refused or path vanished — almost certainly a
            // dead GUI's leftovers. GC.
            let _ = std::fs::remove_file(&path);
        }
    }
    delivered
}

/// [`notify_refresh`] for a single explicit socket path. Exposed for
/// tests. Returns `true` iff the payload was written successfully.
pub fn notify_refresh_at(path: &Path) -> bool {
    let Ok(mut stream) = UnixStream::connect(path) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(CONNECT_TIMEOUT));
    stream.write_all(REFRESH_PAYLOAD).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    /// Bump on every socket path we hand out so concurrent tests
    /// inside the same temp dir can't collide.
    static FAKE_PID: AtomicUsize = AtomicUsize::new(1);

    fn fake_socket_path(dir: &Path) -> PathBuf {
        let pid = FAKE_PID.fetch_add(1, Ordering::Relaxed);
        dir.join(format!("{}test{pid}.{SOCKET_EXTENSION}", socket_prefix()))
    }

    /// Spawn a thread that accepts one connection on `listener` and
    /// returns whatever payload the peer sent.
    fn spawn_drain(listener: UnixListener) -> thread::JoinHandle<Vec<u8>> {
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).unwrap();
            buf
        })
    }

    // ── path-scoped helper ────────────────────────────────────────

    #[test]
    fn notify_at_returns_false_when_no_listener() {
        let tmp = TempDir::new().unwrap();
        let path = fake_socket_path(tmp.path());
        assert!(!notify_refresh_at(&path));
    }

    #[test]
    fn notify_at_delivers_to_listener() {
        let tmp = TempDir::new().unwrap();
        let path = fake_socket_path(tmp.path());

        let listener = UnixListener::bind(&path).unwrap();
        let handle = spawn_drain(listener);

        thread::sleep(Duration::from_millis(50));
        assert!(notify_refresh_at(&path));
        assert_eq!(handle.join().unwrap(), REFRESH_PAYLOAD);
    }

    // ── directory-scoped enumeration ──────────────────────────────

    #[test]
    fn notify_in_returns_zero_for_empty_dir() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(notify_refresh_in(tmp.path()), 0);
    }

    #[test]
    fn notify_in_returns_zero_for_missing_dir() {
        // No `read_dir` blow-up: the runtime dir might not exist on
        // headless containers / first boot.
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        assert_eq!(notify_refresh_in(&missing), 0);
    }

    #[test]
    fn notify_in_delivers_to_every_listener() {
        // Three concurrent GUIs in the same runtime dir → one notify
        // pass → all three drain the payload.
        let tmp = TempDir::new().unwrap();
        let mut handles = Vec::new();
        for _ in 0..3 {
            let path = fake_socket_path(tmp.path());
            let listener = UnixListener::bind(&path).unwrap();
            handles.push(spawn_drain(listener));
        }

        thread::sleep(Duration::from_millis(50));
        assert_eq!(notify_refresh_in(tmp.path()), 3);
        for h in handles {
            assert_eq!(h.join().unwrap(), REFRESH_PAYLOAD);
        }
    }

    #[test]
    fn notify_in_garbage_collects_stale_sockets_and_keeps_live_ones() {
        // Mix one live listener with two stale socket files (regular
        // files at the socket path, mimicking a crashed GUI on a
        // filesystem that doesn't auto-clean). After the pass the
        // stale files should be gone and the live one untouched.
        let tmp = TempDir::new().unwrap();

        let stale_a = fake_socket_path(tmp.path());
        let stale_b = fake_socket_path(tmp.path());
        std::fs::write(&stale_a, b"").unwrap();
        std::fs::write(&stale_b, b"").unwrap();

        let live_path = fake_socket_path(tmp.path());
        let listener = UnixListener::bind(&live_path).unwrap();
        let handle = spawn_drain(listener);

        thread::sleep(Duration::from_millis(50));
        let delivered = notify_refresh_in(tmp.path());
        assert_eq!(delivered, 1, "only the live listener should receive");
        assert!(!stale_a.exists(), "stale socket A should be GC'd");
        assert!(!stale_b.exists(), "stale socket B should be GC'd");
        assert!(live_path.exists(), "live socket must not be GC'd");
        assert_eq!(handle.join().unwrap(), REFRESH_PAYLOAD);
    }

    #[test]
    fn notify_in_skips_files_outside_the_user_prefix() {
        // A file owned by a different (fake) user must not be touched
        // — neither connected to nor unlinked. This guards against
        // cross-user interference on shared hosts.
        let tmp = TempDir::new().unwrap();
        let other_user = tmp.path().join("modde-99999-pid42.sock");
        std::fs::write(&other_user, b"").unwrap();

        let delivered = notify_refresh_in(tmp.path());
        assert_eq!(delivered, 0);
        assert!(other_user.exists(), "other-user file must be left alone");
    }

    #[test]
    fn notify_in_skips_files_with_other_extensions() {
        // A `.lock` or `.tmp` file with our prefix shouldn't be
        // mistaken for a socket. (Same prefix, wrong suffix.)
        let tmp = TempDir::new().unwrap();
        let lock = tmp.path().join(format!("{}pid1.lock", socket_prefix()));
        std::fs::write(&lock, b"").unwrap();

        assert_eq!(notify_refresh_in(tmp.path()), 0);
        assert!(lock.exists(), "non-socket files must be left alone");
    }

    // ── helpers ───────────────────────────────────────────────────

    #[test]
    fn cleanup_socket_removes_file() {
        let tmp = TempDir::new().unwrap();
        let path = fake_socket_path(tmp.path());
        std::fs::write(&path, b"").unwrap();
        assert!(path.exists());
        cleanup_socket(&path);
        assert!(!path.exists());
    }

    #[test]
    fn cleanup_socket_is_idempotent() {
        // Calling on a non-existent path must not panic — drop guards
        // can fire after explicit cleanup.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("never-existed.sock");
        cleanup_socket(&path);
        cleanup_socket(&path); // second call: still fine
    }

    #[test]
    fn gui_socket_path_includes_pid() {
        // Path layout is load-bearing — change it and you break
        // every running GUI's socket. Lock it in.
        let path = gui_socket_path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let prefix = socket_prefix();
        assert!(name.starts_with(&prefix), "expected prefix {prefix} in {name}");
        assert!(name.ends_with(".sock"), "expected .sock suffix in {name}");
        let pid = std::process::id().to_string();
        assert!(name.contains(&pid), "expected pid {pid} in {name}");
    }
}
