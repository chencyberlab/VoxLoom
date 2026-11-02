//! Keyword search over transcripts. Every word of a query must appear in a
//! row; `*` stands for any letters and `?` for one (`budg*`, `*port`,
//! `organi?ation`), and "quoted words" must appear together in that order.
//! Words are compared the way FTS5's default tokenizer reads them: runs of
//! letters and digits, ignoring case and accents, so `don't` is the phrase
//! "don t". FTS5 narrows the rows to read; `find` decides, and returns the
//! matched words for the page to highlight.
use crate::{db::Db, err, Result};
use rusqlite::{params_from_iter, types::Value as Sql};
use serde_json::{json, Value};

/// At most this many moments are returned, newest recording first.
pub const LIMIT: usize = 100;
const TERMS: usize = 24;

type Word = Vec<char>;

/// Lower case without accents, exactly as unicode61 folds letters: the
/// index keeps whatever it does not fold, so a letter folded here but kept
/// there is never found, even spelled as transcribed (issue #18: Søren,
/// Łukasz). That is why ø ł đ ħ ı ŀ ŉ ŧ, which have no accent to remove,
/// stay as they are. English transcripts rarely need more than Latin-1 and
/// Latin Extended-A; `every_folded_letter_is_found_both_ways` checks each.
fn fold(c: char) -> char {
    const GROUPS: [(&str, char); 19] = [
        ("àáâãäåāăą", 'a'),
        ("çćĉċč", 'c'),
        ("ď", 'd'),
        ("èéêëēĕėęě", 'e'),
        ("ĝğġģ", 'g'),
        ("ĥ", 'h'),
        ("ìíîïĩīĭį", 'i'),
        ("ĵ", 'j'),
        ("ķ", 'k'),
        ("ĺļľ", 'l'),
        ("ñńņň", 'n'),
        ("òóôõöōŏő", 'o'),
        ("ŕŗř", 'r'),
        ("śŝşš", 's'),
        ("ţť", 't'),
        ("ùúûüũūŭůűų", 'u'),
        ("ŵ", 'w'),
        ("ýÿŷ", 'y'),
        ("źżž", 'z'),
    ];
    if c.is_ascii() {
        return c;
    }
    GROUPS
        .iter()
        .find(|(group, _)| group.contains(c))
        .map_or(c, |(_, base)| *base)
}

fn letters(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().flat_map(char::to_lowercase).map(fold)
}

fn wild(c: &char) -> bool {
    matches!(c, '*' | '?')
}

/// The query's terms: each one a word, or the words of a quoted phrase. A
/// quote left open runs to the end. A word of wildcards alone would match
/// every row and is dropped; inside a phrase it stands for any one word.
fn parse(query: &str) -> Vec<Vec<Word>> {
    let words = |text: &str| -> Vec<Word> {
        text.split(|c: char| !(c.is_alphanumeric() || wild(&c)))
            .filter(|w| !w.is_empty())
            .map(|w| letters(w).collect())
            .collect()
    };
    let mut terms = vec![];
    for (i, part) in query.split('"').enumerate() {
        if i % 2 == 1 {
            terms.push(words(part));
        } else {
            terms.extend(part.split_whitespace().map(words));
        }
    }
    terms.retain(|t| t.iter().any(|w| w.iter().any(|c| !wild(c))));
    terms.truncate(TERMS);
    terms
}

/// The row's words with where each starts and ends, in UTF-16 units: the
/// page's string offsets.
fn tokens(text: &str) -> Vec<(Word, usize, usize)> {
    let (mut out, mut word, mut start, mut at) = (vec![], vec![], 0, 0);
    for c in text.chars() {
        if c.is_alphanumeric() {
            if word.is_empty() {
                start = at;
            }
            word.extend(c.to_lowercase().map(fold));
        } else if !word.is_empty() {
            out.push((std::mem::take(&mut word), start, at));
        }
        at += c.len_utf16();
    }
    if !word.is_empty() {
        out.push((word, start, at));
    }
    out
}

fn glob(pattern: &[char], word: &[char]) -> bool {
    let (mut p, mut w, mut star, mut from) = (0, 0, None, 0);
    while w < word.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == word[w]) {
            p += 1;
            w += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            from = w;
            p += 1;
        } else if let Some(s) = star {
            p = s + 1;
            from += 1;
            w = from;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

/// Where every term appears in `text`, or None if one does not.
fn find(terms: &[Vec<Word>], text: &str) -> Option<Vec<[usize; 2]>> {
    let tokens = tokens(text);
    let mut marks = vec![];
    for term in terms {
        let before = marks.len();
        for (i, run) in tokens.windows(term.len()).enumerate() {
            if term.iter().zip(run).all(|(p, (w, _, _))| glob(p, w)) {
                marks.push([tokens[i].1, run[run.len() - 1].2]);
            }
        }
        if marks.len() == before {
            return None;
        }
    }
    marks.sort_unstable();
    let mut merged: Vec<[usize; 2]> = vec![];
    for [a, b] in marks {
        match merged.last_mut() {
            Some(last) if a <= last[1] => last[1] = last[1].max(b),
            _ => merged.push([a, b]),
        }
    }
    Some(merged)
}

/// An FTS5 query every matching row also matches: each word that starts with
/// letters, as itself or as a prefix of its letters before a wildcard. Empty
/// when every word starts with a wildcard, and then every row is read.
fn prefilter(terms: &[Vec<Word>]) -> String {
    let mut parts = vec![];
    for word in terms.iter().flatten() {
        let lead: String = word.iter().take_while(|c| !wild(c)).collect();
        if !lead.is_empty() {
            parts.push(format!("\"{lead}\"{}", if lead.chars().count() < word.len() { "*" } else { "" }));
        }
    }
    parts.sort();
    parts.dedup();
    parts.join(" AND ")
}

pub fn keyword(db: &Db, query: &str, filter: &Value) -> Result<Value> {
    let terms = parse(query);
    if terms.is_empty() {
        return Ok(json!({"hits":[],"more":false}));
    }
    let chosen = |key: &str| {
        filter[key]
            .as_str()
            .filter(|s| !s.is_empty() && *s != "all")
            .map(|s| Sql::Text(s.into()))
            .unwrap_or(Sql::Null)
    };
    let narrow = prefilter(&terms);
    let mut sql = String::from("SELECT s.meeting_id,m.title,m.created_at,s.start_ms,s.end_ms,s.text,s.source FROM transcript_segments s JOIN meetings m ON m.id=s.meeting_id WHERE (?1 IS NULL OR s.meeting_id=?1) AND (?2 IS NULL OR s.source=?2) AND (?3 IS NULL OR m.created_at>=?3) AND (?4 IS NULL OR m.created_at<=?4)");
    let mut values = vec![
        chosen("meeting_id"),
        chosen("source"),
        filter["after"].as_i64().map_or(Sql::Null, Sql::Integer),
        filter["before"].as_i64().map_or(Sql::Null, Sql::Integer),
    ];
    if !narrow.is_empty() {
        sql.push_str(" AND s.id IN (SELECT rowid FROM transcript_fts WHERE transcript_fts MATCH ?5)");
        values.push(Sql::Text(narrow));
    }
    sql.push_str(" ORDER BY m.created_at DESC,m.id,s.start_ms,s.id");
    let mut stmt = db.conn.prepare(&sql).map_err(err)?;
    let mut rows = stmt.query(params_from_iter(values)).map_err(err)?;
    let mut hits = vec![];
    while let Some(r) = rows.next().map_err(err)? {
        let text: String = r.get(5).map_err(err)?;
        let Some(marks) = find(&terms, &text) else {
            continue;
        };
        if hits.len() == LIMIT {
            return Ok(json!({"hits":hits,"more":true}));
        }
        hits.push(json!({"meeting_id":r.get::<_,String>(0).map_err(err)?,"title":r.get::<_,String>(1).map_err(err)?,"created_at":r.get::<_,i64>(2).map_err(err)?,"start_ms":r.get::<_,i64>(3).map_err(err)?,"end_ms":r.get::<_,i64>(4).map_err(err)?,"text":text,"source":r.get::<_,String>(6).map_err(err)?,"marks":marks}));
    }
    Ok(json!({"hits":hits,"more":false}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn marks(query: &str, text: &str) -> Option<Vec<String>> {
        let units: Vec<u16> = text.encode_utf16().collect();
        find(&parse(query), text).map(|m| {
            m.iter()
                .map(|[a, b]| String::from_utf16(&units[*a..*b]).unwrap())
                .collect()
        })
    }

    #[test]
    fn words_wildcards_and_phrases() {
        let row = "The Budget report was approved, and we'll re-port it to Zoë's café.";
        assert_eq!(marks("budget", row).unwrap(), ["Budget"]);
        assert_eq!(marks("BUDGET approved", row).unwrap(), ["Budget", "approved"]);
        assert!(marks("budget invoice", row).is_none(), "every word must appear");
        assert!(marks("budg", row).is_none(), "a word without a wildcard is whole");
        assert_eq!(marks("budg*", row).unwrap(), ["Budget"]);
        assert_eq!(marks("*port", row).unwrap(), ["report", "port"]);
        assert_eq!(marks("r*t", row).unwrap(), ["report"]);
        assert_eq!(marks("appro?ed", row).unwrap(), ["approved"]);
        assert!(marks("appro?d", row).is_none(), "? is exactly one letter");
        assert_eq!(marks("\"budget report\"", row).unwrap(), ["Budget report"]);
        assert!(marks("\"report budget\"", row).is_none(), "a phrase keeps its order");
        assert_eq!(marks("\"the * report\"", row).unwrap(), ["The Budget report"]);
        assert_eq!(marks("\"was approv*", row).unwrap(), ["was approved"], "an open quote runs to the end");
        assert_eq!(marks("we'll", row).unwrap(), ["we'll"]);
        assert_eq!(marks("re-port", row).unwrap(), ["re-port"]);
        assert_eq!(marks("zoe cafe", row).unwrap(), ["Zoë", "café"], "accents are ignored");
        assert_eq!(marks("ZOË", row).unwrap(), ["Zoë"]);
        assert!(parse("* ?? \"*\"").is_empty(), "wildcards alone match nothing");
        assert_eq!(marks("budget *", row).unwrap(), ["Budget"]);
    }

    #[test]
    fn marks_are_page_offsets() {
        // UTF-16 units, as JavaScript slices strings: the emoji takes two.
        let units: Vec<u16> = "🎙 café budget".encode_utf16().collect();
        let found = find(&parse("budget"), "🎙 café budget").unwrap();
        assert_eq!(found, [[8, 14]]);
        assert_eq!(String::from_utf16(&units[8..14]).unwrap(), "budget");
        assert_eq!(find(&parse("bud* budget"), "budget").unwrap(), [[0, 6]], "overlaps merge");
    }

    #[test]
    fn prefilter_never_excludes_a_match() {
        assert_eq!(prefilter(&parse("Budget r*t \"the * report\" *port")), "\"budget\" AND \"r\"* AND \"report\" AND \"the\"");
        assert_eq!(prefilter(&parse("*port")), "");
    }

    fn library() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("library.db")).unwrap();
        db.create("busy", "Busy", 20000, "ready").unwrap();
        db.create("quiet", "Quiet", 10000, "ready").unwrap();
        // 600 rows in the newer recording match; the older one's single row
        // must still be found when it is the recording searched.
        for i in 0..600 {
            db.conn.execute("INSERT INTO transcript_segments(meeting_id,start_ms,end_ms,text,source) VALUES('busy',?,?,'the budget was approved again','mic')", params![i * 1000, i * 1000 + 900]).unwrap();
        }
        db.conn.execute("INSERT INTO transcript_segments(meeting_id,start_ms,end_ms,text,source) VALUES('quiet',0,900,'Our budgets and the report were approved','system')", []).unwrap();
        db.conn.execute_batch("UPDATE meetings SET created_at=20000 WHERE id='busy'; UPDATE meetings SET created_at=10000 WHERE id='quiet';").unwrap();
        (dir, db)
    }

    #[test]
    fn filters_apply_before_the_limit() {
        let (_dir, db) = library();
        let search = |q: &str, f: Value| keyword(&db, q, &f).unwrap();
        let all = search("budget", json!({"meeting_id":"all","source":"all"}));
        assert_eq!(all["hits"].as_array().unwrap().len(), LIMIT);
        assert_eq!(all["more"], true);
        assert_eq!(all["hits"][0]["meeting_id"], "busy", "newest recording first");
        assert_eq!(all["hits"][1]["start_ms"], 1000, "then in time order");
        let quiet = search("budget*", json!({"meeting_id":"quiet"}));
        assert_eq!(quiet["hits"].as_array().unwrap().len(), 1);
        assert_eq!(quiet["more"], false);
        assert_eq!(quiet["hits"][0]["marks"], json!([[4, 11]]));
        assert_eq!(search("budget", json!({"source":"system"}))["hits"].as_array().unwrap().len(), 0, "budgets is not budget");
        assert_eq!(search("*port", json!({}))["hits"][0]["meeting_id"], "quiet", "a leading wildcard reads every row");
        assert_eq!(search("budget", json!({"after":20001}))["hits"].as_array().unwrap().len(), 0);
        assert_eq!(search("budget*", json!({"before":10000}))["hits"].as_array().unwrap().len(), 1);
        assert!(search("budget", json!({"meeting_id":"missing"}))["hits"].as_array().unwrap().is_empty());
        assert!(search("\"", json!({}))["hits"].as_array().unwrap().is_empty());
        // FTS5 syntax typed by the person is only text.
        assert!(search("budget OR NEAR(x) AND -", json!({}))["hits"].as_array().unwrap().is_empty());
        assert_eq!(search("\"approved again\"", json!({}))["hits"].as_array().unwrap().len(), LIMIT);
    }

    #[test]
    fn names_are_found_as_transcribed() {
        let (_dir, db) = library();
        db.conn.execute("INSERT INTO transcript_segments(meeting_id,start_ms,end_ms,text,source) VALUES('quiet',1000,1900,'Meet Søren, Łukasz and Đặng about the budget at the Café in Malmö.','mic')", []).unwrap();
        let found = |q: &str| keyword(&db, q, &json!({})).unwrap()["hits"].as_array().unwrap().iter().filter(|h| h["start_ms"] == 1000).count();
        for query in ["Søren", "søren", "SØREN", "sør*", "Meet Søren", "\"meet søren\"", "budget Søren", "Łukasz", "łukasz", "Đặng", "đặng", "Café", "cafe", "Malmo", "MALMÖ"] {
            assert_eq!(found(query), 1, "{query}");
        }
        // These letters are not accented forms of o, l or d: the index keeps them.
        assert_eq!(found("soren"), 0);
    }

    #[test]
    fn every_folded_letter_is_found_both_ways() {
        // A letter folded here but kept by the index would hide every word
        // that has it; each one, in capitals too, must be found by itself and
        // by the letter it folds to.
        let (_dir, db) = library();
        let letters: Vec<char> = (0xC0..0x250).filter_map(char::from_u32).filter(|c| fold(*c) != *c).collect();
        assert_eq!(letters.len(), 80);
        for c in letters.iter().copied().chain(letters.iter().flat_map(|c| c.to_uppercase()).filter(|c| !c.is_ascii())) {
            db.conn.execute("DELETE FROM transcript_segments WHERE meeting_id='quiet'", []).unwrap();
            db.conn.execute("INSERT INTO transcript_segments(meeting_id,start_ms,end_ms,text,source) VALUES('quiet',0,900,?,'mic')", [format!("q{c}z")]).unwrap();
            let base = format!("q{}z", c.to_lowercase().map(fold).collect::<String>());
            for query in [format!("q{c}z"), base] {
                assert_eq!(keyword(&db, &query, &json!({})).unwrap()["hits"].as_array().unwrap().len(), 1, "{c}: {query}");
            }
        }
    }

    #[test]
    fn edited_rows_are_found_by_their_new_words() {
        let (_dir, db) = library();
        db.conn.execute("UPDATE transcript_segments SET text='Invoice sent' WHERE meeting_id='quiet'", []).unwrap();
        assert_eq!(keyword(&db, "invoice", &json!({})).unwrap()["hits"].as_array().unwrap().len(), 1);
        assert_eq!(keyword(&db, "report", &json!({})).unwrap()["hits"].as_array().unwrap().len(), 0);
    }
}
