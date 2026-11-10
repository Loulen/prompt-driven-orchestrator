//! The period a Stats series is bucketed by.

/// Map a bucket granularity to its SQLite `strftime` format. `None` for an
/// unknown granularity (the handler answers `400`).
pub fn strftime_fmt(bucket: &str) -> Option<&'static str> {
    match bucket {
        "day" => Some("%Y-%m-%d"),
        "week" => Some("%Y-W%W"),
        "month" => Some("%Y-%m"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strftime_fmt_maps_known_buckets_only() {
        assert_eq!(strftime_fmt("day"), Some("%Y-%m-%d"));
        assert_eq!(strftime_fmt("week"), Some("%Y-W%W"));
        assert_eq!(strftime_fmt("month"), Some("%Y-%m"));
        assert_eq!(strftime_fmt("year"), None);
        assert_eq!(strftime_fmt(""), None);
    }
}
