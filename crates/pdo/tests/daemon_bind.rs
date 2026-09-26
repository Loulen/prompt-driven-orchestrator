//! Layer 3a coverage for the daemon bind and precedence contract in #769.

use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn unused_loopback_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve loopback port");
    listener.local_addr().unwrap().port()
}

#[test]
fn daemon_bind_uses_the_environment_and_the_flag_takes_precedence() {
    let cases: &[(&[&str], &str)] = &[(&[], "127.0.0.1"), (&["--bind", "127.0.0.1"], "0.0.0.0")];

    for (extra_args, env_bind) in cases {
        let port = unused_loopback_port();
        let tempdir = tempfile::tempdir().expect("tempdir");
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_pdo"));
        command
            .current_dir(tempdir.path())
            .args(["daemon", "--port"])
            .arg(port.to_string())
            .args(*extra_args)
            .env("PDO_BIND", env_bind)
            .env("PDO_PRICE_SYNC", "off")
            .env("PDO_DAEMON_NO_CLEANUP", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().expect("failed to spawn pdo daemon");

        let stderr = child.stderr.take().expect("stderr pipe missing");
        let output = Arc::new(Mutex::new(String::new()));
        let writer = Arc::clone(&output);
        let reader = std::thread::spawn(move || {
            let mut stderr = stderr;
            let mut chunk = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                if let Ok(mut text) = writer.lock() {
                    text.push_str(&String::from_utf8_lossy(&chunk[..n]));
                }
            }
        });
        let expected = format!("PDO daemon listening on 127.0.0.1:{port}");
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while std::time::Instant::now() < deadline
            && !output.lock().is_ok_and(|text| text.contains(&expected))
        {
            std::thread::sleep(Duration::from_millis(100));
        }

        let response = reqwest::blocking::get(format!("http://127.0.0.1:{port}/sessions"));
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        let collected = output.lock().map(|text| text.clone()).unwrap_or_default();

        assert!(
            collected.contains(&expected),
            "daemon did not bind to the requested address; stderr:\n{collected}"
        );
        assert!(
            response.is_ok_and(|r| r.status().is_success()),
            "daemon did not answer on its requested bind address"
        );
    }
}
