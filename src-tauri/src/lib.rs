pub mod audio;
pub mod db;
pub mod metrics;
pub mod platform;
pub mod process;
pub mod search;
pub mod service;

pub type Result<T> = std::result::Result<T, String>;
pub fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

pub fn timestamp(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

pub fn meeting_notes(text: &str) -> String {
    fn timecode(value: &str) -> bool {
        value.len() == 8
            && value.bytes().enumerate().all(|(i, c)| {
                if i == 2 || i == 5 {
                    c == b':'
                } else {
                    c.is_ascii_digit()
                }
            })
    }
    text.lines()
        .map(|line| {
            let mut cleaned = String::new();
            let mut remaining = line;
            while let Some(start) = remaining.find('[') {
                cleaned.push_str(&remaining[..start]);
                let rest = &remaining[start..];
                if let Some(end) = rest.find(']') {
                    if timecode(&rest[1..end]) {
                        remaining = &rest[end + 1..];
                        continue;
                    }
                }
                cleaned.push('[');
                remaining = &rest[1..];
            }
            cleaned.push_str(remaining);
            let trimmed = cleaned.trim_start();
            let (bullet, body) = if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                (&trimmed[..2], trimmed[2..].trim_start())
            } else {
                ("", trimmed)
            };
            if body.get(..8).is_some_and(timecode) {
                cleaned = format!(
                    "{}{}",
                    bullet,
                    body[8..].trim_start_matches([' ', '-', '–', '—', ':'])
                );
            }
            cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

pub fn chunks(duration_ms: i64, window_ms: i64, overlap_ms: i64) -> Vec<(i64, i64)> {
    if duration_ms <= 0 || window_ms <= overlap_ms || overlap_ms < 0 {
        return vec![];
    }
    let mut out = vec![];
    let mut start = 0;
    while start < duration_ms {
        let end = (start + window_ms).min(duration_ms);
        out.push((start, end));
        if end == duration_ms {
            break;
        }
        start = end - overlap_ms;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn meeting_notes_export_removes_source_codes_and_preserves_deadlines() {
        assert_eq!(meeting_notes("## Actions\n- [00:00:03] Sarah sends the invoice on Friday at 09:30.\n00:00:14 — Review [draft]"),"## Actions\n- Sarah sends the invoice on Friday at 09:30.\nReview [draft]");
    }
    #[test]
    fn chunk_offsets_and_tail() {
        assert_eq!(
            chunks(65000, 30000, 2000),
            vec![(0, 30000), (28000, 58000), (56000, 65000)]
        );
        assert!(chunks(10, 2, 2).is_empty());
        assert!(chunks(0, 30000, 2000).is_empty());
        assert_eq!(timestamp(3661000), "01:01:01");
    }
}
