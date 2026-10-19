//! Searching one transcript's index: the entries whose visible text holds the
//! query, each with an excerpt around its first match.
//!
//! What a search reads is what the Transcript tab shows folded: a message's
//! text, a tool call's one-line summary. A tool call's input and output stay in
//! the file, and the model's thinking was never indexed.

use aho_corasick::AhoCorasick;
use serde::Serialize;
use ts_rs::TS;

use super::index::{Indexed, TranscriptIndex};
use super::Speaker;

/// How many characters of context an excerpt keeps before its match.
pub const SNIPPET_BEFORE: usize = 48;
/// How many characters of context an excerpt keeps after its match.
pub const SNIPPET_AFTER: usize = 96;

/// A search query, compiled once: matching ignores case. An ASCII query (the
/// usual one: a word, a path, a command) runs a case-insensitive automaton on
/// the bytes, with no copy of the text; any other folds the text char by char.
#[derive(Debug, Clone)]
pub struct Needle {
    folded: Vec<char>,
    ascii: Option<AhoCorasick>,
    /// The automaton of the folded query, run on a folded copy of the text
    /// when the query is not ASCII.
    unicode: Option<AhoCorasick>,
}

impl Needle {
    /// The needle of `query`, or `None` for a blank query.
    pub fn new(query: &str) -> Option<Self> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return None;
        }
        let ascii = trimmed
            .is_ascii()
            .then(|| {
                AhoCorasick::builder()
                    .ascii_case_insensitive(true)
                    .build([trimmed])
                    .ok()
            })
            .flatten();
        let folded: Vec<char> = trimmed.chars().map(fold).collect();
        let unicode = ascii
            .is_none()
            .then(|| AhoCorasick::new([folded.iter().collect::<String>()]).ok())
            .flatten();
        Some(Needle {
            folded,
            ascii,
            unicode,
        })
    }

    /// Whether `text` holds the needle: what counting the matches asks, in one
    /// pass over the text.
    pub fn matches(&self, text: &str) -> bool {
        match (&self.ascii, &self.unicode) {
            (Some(ac), _) => ac.is_match(text),
            (None, Some(ac)) => ac.is_match(&text.chars().map(fold).collect::<String>()),
            (None, None) => self.find(text).is_some(),
        }
    }

    /// The byte range of the first match of the needle in `text`.
    pub fn find(&self, text: &str) -> Option<(usize, usize)> {
        if let Some(ac) = &self.ascii {
            return ac.find(text).map(|m| (m.start(), m.end()));
        }
        let hay: Vec<(usize, char)> = text.char_indices().map(|(i, c)| (i, fold(c))).collect();
        let n = self.folded.len();
        if hay.len() < n {
            return None;
        }
        (0..=hay.len() - n)
            .find(|&at| {
                hay[at..at + n]
                    .iter()
                    .map(|(_, c)| *c)
                    .eq(self.folded.iter().copied())
            })
            .map(|at| {
                let end = hay.get(at + n).map_or(text.len(), |(i, _)| *i);
                (hay[at].0, end)
            })
    }
}

/// One character, case-folded when that keeps it one character.
fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// The text around a match, on one line: `before` and `after` are cut at
/// [`SNIPPET_BEFORE`] and [`SNIPPET_AFTER`] characters, with `…` where cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "TranscriptSnippet")]
pub struct Snippet {
    pub before: String,
    pub matched: String,
    pub after: String,
}

/// The excerpt of `text` around the byte range `range` (on char boundaries,
/// as [`Needle::find`] answers it).
pub fn snippet(text: &str, (start, end): (usize, usize)) -> Snippet {
    let head = &text[..start];
    let mut kept: Vec<char> = head.chars().rev().take(SNIPPET_BEFORE + 1).collect();
    let cut_before = kept.len() > SNIPPET_BEFORE;
    kept.truncate(SNIPPET_BEFORE);
    kept.reverse();
    let mut before = one_line(kept.into_iter());
    if cut_before {
        before = format!("…{}", before.trim_start());
    }
    let tail = &text[end..];
    let mut after = one_line(tail.chars().take(SNIPPET_AFTER));
    if tail.chars().nth(SNIPPET_AFTER).is_some() {
        after = format!("{}…", after.trim_end());
    }
    Snippet {
        before,
        matched: one_line(text[start..end].chars()),
        after,
    }
}

fn one_line(chars: impl Iterator<Item = char>) -> String {
    chars
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect()
}

/// An entry of a transcript that holds the query.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, rename = "TranscriptHit")]
pub enum TranscriptHit {
    Message {
        /// The entry's position in the transcript, as a page numbers it.
        index: u32,
        speaker: Speaker,
        snippet: Snippet,
        #[ts(optional)]
        #[serde(skip_serializing_if = "Option::is_none")]
        at: Option<String>,
    },
    Tool {
        index: u32,
        name: String,
        /// The excerpt of the call's one-line summary.
        snippet: Snippet,
        #[ts(optional)]
        #[serde(skip_serializing_if = "Option::is_none")]
        at: Option<String>,
    },
}

/// What a search found in one transcript.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IndexSearch {
    /// The first matching entries, in transcript order, at most the asked number.
    pub hits: Vec<TranscriptHit>,
    /// Every matching entry, beyond the ones kept.
    pub matched: u32,
    /// The search stopped before the end: it was cancelled.
    pub cancelled: bool,
}

impl TranscriptIndex {
    /// The entries holding `needle`, in order: every one counted, the first
    /// `keep` with their excerpt. `cancelled` is asked between entries; once it
    /// answers `true` the search stops where it is.
    pub fn search(
        &self,
        needle: &Needle,
        keep: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> IndexSearch {
        let mut found = IndexSearch::default();
        for (i, entry) in self.entries.iter().enumerate() {
            if cancelled() {
                found.cancelled = true;
                break;
            }
            let text = match &entry.body {
                Indexed::Message { text, .. } => text,
                Indexed::Tool { summary, .. } => summary,
            };
            if !needle.matches(text) {
                continue;
            }
            found.matched += 1;
            if found.hits.len() >= keep {
                continue;
            }
            let Some(range) = needle.find(text) else {
                continue;
            };
            let index = i as u32;
            let at = entry.at.clone();
            let snippet = snippet(text, range);
            found.hits.push(match &entry.body {
                Indexed::Message { speaker, .. } => TranscriptHit::Message {
                    index,
                    speaker: *speaker,
                    snippet,
                    at,
                },
                Indexed::Tool { name, .. } => TranscriptHit::Tool {
                    index,
                    name: name.clone(),
                    snippet,
                    at,
                },
            });
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::probes;
    use crate::domain::registry::CLAUDE;

    const CLAUDE_FIXTURE: &str = include_str!("../../../tests/fixtures/transcript/claude.jsonl");

    fn index(text: &str) -> TranscriptIndex {
        let mut index = TranscriptIndex::new();
        index.feed(text.as_bytes(), |line| {
            probes::transcript_records(CLAUDE, line)
        });
        index
    }

    fn never() -> bool {
        false
    }

    #[test]
    fn a_blank_query_is_no_needle() {
        assert!(Needle::new("  \n").is_none());
    }

    #[test]
    fn matching_ignores_case_and_keeps_the_original_range() {
        let needle = Needle::new(" DeBounce ").unwrap();
        assert_eq!(needle.find("Add a debounce."), Some((6, 14)));
        assert_eq!(needle.find("Éé DEBOUNCE"), Some((5, 13)), "byte offsets");
        assert_eq!(needle.find("debounc"), None);

        // A query beyond ASCII folds its accents' case too.
        let accented = Needle::new("été").unwrap();
        assert_eq!(accented.find("Un ÉTÉ chaud"), Some((3, 8)));
        assert_eq!(accented.find("Un ETE chaud"), None);
        assert!(accented.matches("Un ÉTÉ chaud") && !accented.matches("Un ETE chaud"));
        assert!(needle.matches("x DEBOUNCE y") && !needle.matches("debounc"));
        let s = snippet("Un ÉTÉ chaud", (3, 8));
        assert_eq!(
            (s.before.as_str(), s.matched.as_str(), s.after.as_str()),
            ("Un ", "ÉTÉ", " chaud")
        );
    }

    #[test]
    fn an_excerpt_cuts_on_one_line_around_the_match() {
        let text = format!("{}\nthe debounce\tis {}", "a".repeat(60), "b".repeat(200));
        let s = snippet(&text, Needle::new("debounce").unwrap().find(&text).unwrap());
        assert_eq!(s.matched, "debounce");
        assert!(s.before.starts_with('…'));
        assert!(s.before.ends_with(" the "));
        assert!(s.after.starts_with(" is "));
        assert!(s.after.ends_with('…'));
        assert!(!s.after.contains('\t'));
        assert!(s.before.chars().count() <= SNIPPET_BEFORE + 1);
        assert!(s.after.chars().count() <= SNIPPET_AFTER + 1);

        let short = snippet("a debounce", (2, 10));
        assert_eq!((short.before.as_str(), short.after.as_str()), ("a ", ""));
    }

    #[test]
    fn a_search_finds_messages_and_tool_summaries_in_order() {
        let found = index(CLAUDE_FIXTURE).search(&Needle::new("debounce").unwrap(), 10, &never);
        let shape: Vec<String> = found
            .hits
            .iter()
            .map(|h| match h {
                TranscriptHit::Message { index, speaker, .. } => format!("{index}:{speaker:?}"),
                TranscriptHit::Tool { index, name, .. } => format!("{index}:{name}"),
            })
            .collect();
        assert_eq!(shape, vec!["2:Agent", "4:Agent"]);
        assert_eq!(found.matched, 2);

        let tools = index(CLAUDE_FIXTURE).search(&Needle::new("SEARCH.ts").unwrap(), 10, &never);
        assert!(matches!(
            tools.hits.as_slice(),
            [TranscriptHit::Tool { index: 6, name, snippet, .. }]
                if name == "Edit" && snippet.matched == "search.ts"
        ));
    }

    #[test]
    fn the_hits_kept_are_bounded_the_count_is_not() {
        let found = index(CLAUDE_FIXTURE).search(&Needle::new("e").unwrap(), 2, &never);
        assert_eq!(found.hits.len(), 2);
        assert_eq!(found.matched, 6);
        assert!(!found.cancelled);
    }

    #[test]
    fn a_cancelled_search_stops_between_entries() {
        let asked = std::cell::Cell::new(0);
        let cancel_on_third = || {
            asked.set(asked.get() + 1);
            asked.get() > 2
        };
        let found = index(CLAUDE_FIXTURE).search(&Needle::new("e").unwrap(), 10, &cancel_on_third);
        assert!(found.cancelled);
        assert_eq!(found.matched, 2);
    }

    #[test]
    fn thinking_is_never_searched() {
        let line = r#"{"type":"assistant","uuid":"a","message":{"role":"assistant","content":[{"type":"thinking","thinking":"SECRET plan"},{"type":"text","text":"Done."}]}}"#;
        let found = index(&format!("{line}\n")).search(&Needle::new("secret").unwrap(), 10, &never);
        assert_eq!(found.matched, 0);
    }
}
