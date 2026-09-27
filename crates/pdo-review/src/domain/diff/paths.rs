//! The paths a patch names: git's markers, its C-quoting, its headers.

/// The path of a `--- `/`+++ ` marker: strip git's trailing tab (paths with
/// spaces), C-unquote, `/dev/null` → `None`, drop the `a/`/`b/` prefix.
pub(super) fn marker_path(rest: &str, prefix: &str) -> Option<String> {
    let rest = rest.strip_suffix('\t').unwrap_or(rest);
    let path = decode_path(rest)?;
    if path == "/dev/null" {
        return None;
    }
    Some(
        path.strip_prefix(prefix)
            .map(str::to_string)
            .unwrap_or(path),
    )
}

pub(super) fn strip_prefix_or_null(token: &str, prefix: &str) -> Option<String> {
    let path = decode_path(token)?;
    if path == "/dev/null" {
        return None;
    }
    Some(
        path.strip_prefix(prefix)
            .map(str::to_string)
            .unwrap_or(path),
    )
}

/// Last resort for a chunk with neither markers nor rename lines (a mode-only
/// change): `diff --git a/X b/X` where both halves name the same path.
pub(super) fn header_path(header: &str) -> Option<String> {
    let rest = header.strip_prefix("diff --git ")?;
    let bytes = rest.as_bytes();
    if bytes.len() % 2 == 0 {
        return None;
    }
    let mid = bytes.len() / 2;
    if bytes[mid] != b' ' {
        return None;
    }
    let a = &rest[..mid];
    let b = &rest[mid + 1..];
    let a = a.strip_prefix("a/")?;
    let b = b.strip_prefix("b/")?;
    if a == b {
        Some(a.to_string())
    } else {
        None
    }
}

/// C-unquote a git path token when it is wrapped in double quotes (the
/// `core.quotepath` output, or a path with control characters), else verbatim.
/// `None` only for an empty token.
pub(super) fn decode_path(token: &str) -> Option<String> {
    if token.is_empty() {
        return None;
    }
    if token.len() >= 2 && token.starts_with('"') && token.ends_with('"') {
        return Some(c_unquote(&token[1..token.len() - 1]));
    }
    Some(token.to_string())
}

/// Octal escapes are raw UTF-8 bytes, so the bytes are collected first and
/// decoded once — a char-by-char replace would mojibake multi-byte codepoints.
fn c_unquote(inner: &str) -> String {
    let mut bytes: Vec<u8> = Vec::with_capacity(inner.len());
    let src = inner.as_bytes();
    let mut i = 0;
    while i < src.len() {
        let c = src[i];
        if c != b'\\' {
            bytes.push(c);
            i += 1;
            continue;
        }
        let Some(&n) = src.get(i + 1) else {
            break; // trailing lone backslash: dropped
        };
        if (b'0'..=b'7').contains(&n) {
            let mut j = i + 1;
            let mut oct = 0u32;
            let mut digits = 0;
            while j < src.len() && digits < 3 && (b'0'..=b'7').contains(&src[j]) {
                oct = oct * 8 + u32::from(src[j] - b'0');
                j += 1;
                digits += 1;
            }
            bytes.push((oct & 0xff) as u8);
            i = j;
            continue;
        }
        let mapped = match n {
            b'a' => Some(0x07),
            b'b' => Some(0x08),
            b'f' => Some(0x0c),
            b'n' => Some(0x0a),
            b'r' => Some(0x0d),
            b't' => Some(0x09),
            b'v' => Some(0x0b),
            b'"' => Some(0x22),
            b'\\' => Some(0x5c),
            _ => None,
        };
        match mapped {
            Some(b) => bytes.push(b),
            None => bytes.push(n),
        }
        i += 2;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
