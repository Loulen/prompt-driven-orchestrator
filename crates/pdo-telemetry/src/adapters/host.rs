//! The name of the machine the daemon runs on.

/// The host name, `None` when the system will not say.
pub fn host_name() -> Option<String> {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer outlives the call and its length is the one passed.
    let status = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if status != 0 {
        return None;
    }
    let end = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
    let name = String::from_utf8_lossy(&buffer[..end]).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// The instance's identity on the measures: the host and the daemon's port,
/// so two daemons of one machine stay apart.
pub fn instance_id(port: u16) -> String {
    format!(
        "{}:{port}",
        host_name().unwrap_or_else(|| "localhost".into())
    )
}
