//! Shell quoting for the scripts a session runs.

pub fn sh_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Single-quote a shell word only when it contains a character that isn't safe
/// bare. Keeps the emitted `docker exec …` argv readable (and its golden stable):
/// simple tokens (`docker`, `exec`, `-i`, `PDO_OUTPUT_out=/a/b`, `1000:1000`) stay
/// bare; anything with a space / quote / `$` / glob char is quoted. Used only to
/// splice the `docker exec` argv into the `bash -c` tail string.
pub fn sh_quote_arg(s: &str) -> String {
    let safe = !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'_' | b'-' | b'.' | b'/' | b'=' | b':' | b',' | b'@' | b'+'
                )
        });
    if safe {
        s.to_string()
    } else {
        sh_single_quote(s)
    }
}
