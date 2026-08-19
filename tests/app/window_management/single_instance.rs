use crate::app::single_instance::{acquire, Instance};
use std::sync::mpsc;

/// Per-pid dir, per-test socket filename. Cargo runs tests in the same
/// process (separate threads), so a shared `ipc.sock` would let one test's
/// primary answer another test's acquire; the distinct names keep each test
/// deterministic. The pre-remove also guards against a stale socket file
/// from an earlier run under a reused pid.
fn temp_socket_path(test_name: &str) -> std::path::PathBuf {
    let n = std::process::id();
    let dir = std::env::temp_dir().join(format!("meatshell-si-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("ipc-{test_name}.sock"));
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn first_acquire_becomes_primary_and_second_forwards() {
    let path = temp_socket_path("first_acquire_becomes_primary_and_second_forwards");
    let (tx, rx) = mpsc::channel();

    let instance = acquire(&path).expect("primary acquire");
    let Instance::Primary { listen } = instance else {
        panic!("first acquire must be primary");
    };
    std::thread::spawn(move || {
        listen.spawn(move |msg| {
            let _ = tx.send(msg);
        });
    });

    // Give the accept loop a moment to start.
    std::thread::sleep(std::time::Duration::from_millis(100));

    match acquire(&path).expect("forward acquire") {
        Instance::Forwarded => {}
        Instance::Primary { .. } => panic!("second acquire must forward"),
    }

    let msg = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("primary should receive the message");
    assert_eq!(msg, "new-window");
}

#[test]
fn forwarding_without_primary_fails() {
    let path = temp_socket_path("forwarding_without_primary_fails");
    // No primary bound: acquire must become primary, not Forwarded.
    match acquire(&path).expect("acquire") {
        Instance::Primary { .. } => {}
        Instance::Forwarded => panic!("no primary to forward to"),
    }
}
