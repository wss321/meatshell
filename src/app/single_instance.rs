// Single-instance coordination. All OS entry points (Windows jump list,
// macOS dock menu, Linux desktop action) launch `meatshell --new-window`.
// The first running instance owns a local socket under the data dir; later
// launches connect, send "new-window\n" and exit, and the primary opens the
// new window in-process (Chrome-style).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(windows)]
use std::os::windows::net::{UnixListener, UnixStream};

const MSG_NEW_WINDOW: &str = "new-window";

pub enum Instance {
    /// This process owns the endpoint. `listen` accepts forwarded requests.
    Primary { listen: Listener },
    /// Another instance is running; the new-window request was forwarded and
    /// this process should exit with success.
    Forwarded,
}

/// Try to become the primary instance; if one already exists, forward a
/// new-window request to it. Never panics on IO trouble — callers treat
/// errors as "just run normally".
pub fn acquire(socket_path: &Path) -> std::io::Result<Instance> {
    match UnixListener::bind(socket_path) {
        Ok(listener) => Ok(Instance::Primary {
            listen: Listener { listener },
        }),
        Err(_) if cfg!(windows) => {
            // On Windows a leftover socket file from a crash blocks bind.
            // If connecting fails too, remove the stale file and retry once.
            if UnixStream::connect(socket_path).is_err() {
                let _ = std::fs::remove_file(socket_path);
                if let Ok(listener) = UnixListener::bind(socket_path) {
                    return Ok(Instance::Primary {
                        listen: Listener { listener },
                    });
                }
            }
            // A live primary exists: forward to it.
            forward(socket_path)
        }
        Err(_) => forward(socket_path),
    }
}

fn forward(socket_path: &Path) -> std::io::Result<Instance> {
    let mut stream = UnixStream::connect(socket_path)?;
    stream.write_all(format!("{MSG_NEW_WINDOW}\n").as_bytes())?;
    stream.flush()?;
    Ok(Instance::Forwarded)
}

/// Endpoint path inside the per-user data dir.
pub fn socket_path() -> PathBuf {
    crate::config::data_dir().join("ipc.sock")
}

pub struct Listener {
    listener: UnixListener,
}

impl Listener {
    /// Blocks forever, invoking `on_msg` for every complete line received.
    /// Spawn this on its own thread.
    pub fn spawn<F: FnMut(String) + Send + 'static>(self, mut on_msg: F) {
        for stream in self.listener.incoming().flatten() {
            if let Some(Ok(line)) = BufReader::new(stream).lines().next() {
                on_msg(line);
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/app/window_management/single_instance.rs"]
mod single_instance_tests;
