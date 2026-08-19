// Single-instance coordination. All OS entry points (Windows jump list,
// macOS dock menu, Linux desktop action) launch `meatshell --new-window`.
// The first running instance owns a local endpoint under the data dir; later
// launches connect, send "new-window\n" and exit, and the primary opens the
// new window in-process (Chrome-style).
//
// Transport split: unix uses a unix-domain socket (`ipc.sock`); Windows uses
// a TCP loopback listener on 127.0.0.1 whose port is published in a port
// file (`ipc.port`), because std's Windows unix-socket support is unstable
// (nightly-only, rust-lang/rust#150487). On Windows every `socket_path`
// argument is therefore reinterpreted as the port-file path.

#[cfg(windows)]
use std::net::{SocketAddr, TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(windows)]
use std::time::Duration;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

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
#[cfg(unix)]
pub fn acquire(socket_path: &Path) -> std::io::Result<Instance> {
    match UnixListener::bind(socket_path) {
        Ok(listener) => Ok(Instance::Primary {
            listen: Listener { listener },
        }),
        Err(_) => forward_unix(socket_path),
    }
}

/// `socket_path` is the port-file path here (see module docs).
#[cfg(windows)]
pub fn acquire(socket_path: &Path) -> std::io::Result<Instance> {
    let port_file = socket_path;
    // A live primary? Connect with a short timeout; a stale port file
    // (parse error, refused/timeout connection) is treated as "no primary".
    if let Some(port) = read_port_file(port_file) {
        if forward_tcp(port).is_ok() {
            return Ok(Instance::Forwarded);
        }
    }
    // No live primary: start one on an ephemeral loopback port.
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    // Without the port file nobody can find us; let the caller fall back
    // to a normal launch.
    std::fs::write(port_file, port.to_string())?;
    // Two processes can both see "no primary" and both reach this point.
    // Whoever wrote the port file last wins; re-check and defer if we lost.
    if read_port_file(port_file) != Some(port) {
        drop(listener);
        if let Some(winner) = read_port_file(port_file) {
            if forward_tcp(winner).is_ok() {
                return Ok(Instance::Forwarded);
            }
        }
        return Err(std::io::Error::other("lost single-instance race"));
    }
    Ok(Instance::Primary {
        listen: Listener { listener },
    })
}

#[cfg(unix)]
fn forward_unix(socket_path: &Path) -> std::io::Result<Instance> {
    let mut stream = UnixStream::connect(socket_path)?;
    stream.write_all(format!("{MSG_NEW_WINDOW}\n").as_bytes())?;
    stream.flush()?;
    Ok(Instance::Forwarded)
}

#[cfg(windows)]
fn forward_tcp(port: u16) -> std::io::Result<Instance> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(300))?;
    stream.write_all(format!("{MSG_NEW_WINDOW}\n").as_bytes())?;
    stream.flush()?;
    Ok(Instance::Forwarded)
}

#[cfg(windows)]
fn read_port_file(port_file: &Path) -> Option<u16> {
    std::fs::read_to_string(port_file).ok()?.trim().parse().ok()
}

/// Endpoint path inside the per-user data dir: the unix socket on unix, the
/// TCP port file on Windows (see module docs).
pub fn socket_path() -> PathBuf {
    #[cfg(windows)]
    {
        crate::config::data_dir().join("ipc.port")
    }
    #[cfg(not(windows))]
    {
        crate::config::data_dir().join("ipc.sock")
    }
}

pub struct Listener {
    #[cfg(unix)]
    listener: UnixListener,
    #[cfg(windows)]
    listener: TcpListener,
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
