//! A new Run's id: its start time to the second, then a random suffix.

pub fn generate_run_id() -> String {
    let now = chrono::Utc::now();
    let ts = now.format("%Y%m%d-%H%M%S");
    let short = &uuid::Uuid::new_v4().to_string()[..7];
    format!("{ts}-{short}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_id_format() {
        let id = generate_run_id();
        // Format: YYYYMMDD-HHMMSS-<7char>
        assert!(id.len() >= 22, "run-id too short: {id}");
        assert!(id.contains('-'));
    }
}
