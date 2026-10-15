use crate::{db::Db, err, Result};
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::HashMap;

pub const MODEL_ID: &str = "litert-community/embeddinggemma-2-740m-litert-lm";
pub const REVISION: &str = "24d962e906c7d332c6428e71c9676855024569e2";
pub const DIM: usize = 768;
pub fn normalized(v: &[f32]) -> Result<Vec<f32>> {
    if v.len() != DIM || v.iter().any(|x| !x.is_finite()) {
        return Err("Expected 768 finite vector components".into());
    }
    let norm = v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    if norm < 1e-12 {
        return Err("Embedding is zero".into());
    }
    Ok(v.iter().map(|x| (*x as f64 / norm) as f32).collect())
}
pub fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
pub fn vector(b: &[u8]) -> Result<Vec<f32>> {
    if b.len() != DIM * 4 {
        return Err("Index dimension mismatch; rebuild index".into());
    }
    normalized(
        &b.chunks_exact(4)
            .map(|x| f32::from_le_bytes(x.try_into().unwrap()))
            .collect::<Vec<_>>(),
    )
}
pub fn exact(db: &Db, query: &str, filter: &Value) -> Result<Vec<Value>> {
    let terms = query
        .split_whitespace()
        .take(24)
        .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ");
    if terms.is_empty() {
        return Ok(vec![]);
    }
    let mut stmt=db.conn.prepare("SELECT s.meeting_id,m.title,m.created_at,s.start_ms,s.end_ms,s.text,s.source,bm25(transcript_fts) FROM transcript_fts JOIN transcript_segments s ON s.id=transcript_fts.rowid JOIN meetings m ON m.id=s.meeting_id WHERE transcript_fts MATCH ? ORDER BY bm25(transcript_fts) LIMIT 500").map_err(err)?;
    let rows=stmt.query_map([terms],|r|Ok(json!({"meeting_id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"created_at":r.get::<_,i64>(2)?,"start_ms":r.get::<_,i64>(3)?,"end_ms":r.get::<_,i64>(4)?,"text":r.get::<_,String>(5)?,"source":r.get::<_,String>(6)?,"modality":"text","rank":r.get::<_,f64>(7)?}))).map_err(err)?;
    rows.filter_map(|r| match r {
        Ok(v) if matches(&v, filter) => Some(Ok(v)),
        Ok(_) => None,
        Err(e) => Some(Err(err(e))),
    })
    .take(80)
    .collect()
}
fn matches(item: &Value, f: &Value) -> bool {
    ["meeting_id", "source", "modality"].iter().all(|k| {
        f[*k]
            .as_str()
            .filter(|s| !s.is_empty() && *s != "all")
            .map(|v| item[*k] == v)
            .unwrap_or(true)
    }) && f["title"]
        .as_str()
        .map(|v| {
            item["title"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains(&v.to_lowercase())
        })
        .unwrap_or(true)
        && f["after"]
            .as_i64()
            .map(|v| item["created_at"].as_i64().unwrap_or(0) >= v)
            .unwrap_or(true)
        && f["before"]
            .as_i64()
            .map(|v| item["created_at"].as_i64().unwrap_or(0) <= v)
            .unwrap_or(true)
}
pub fn semantic(db: &Db, query: &[f32], filter: &Value) -> Result<Vec<Value>> {
    let q = normalized(query)?;
    let mut stmt=db.conn.prepare("SELECT i.meeting_id,m.title,m.created_at,i.start_ms,i.end_ms,i.text,i.modality,v.vector,COALESCE(t.source,'mixed'),i.revision,i.dimensions FROM embedding_items i JOIN embedding_vectors v ON v.item_id=i.id JOIN meetings m ON m.id=i.meeting_id LEFT JOIN audio_tracks t ON t.id=i.track_id WHERE i.model_id=? AND i.backend='litert-lm-cpu' AND i.chunk_version=1").map_err(err)?;
    let mut results = vec![];
    let mut rows = stmt.query(params![MODEL_ID]).map_err(err)?;
    while let Some(row) = rows.next().map_err(err)? {
        if row.get::<_, String>(9).map_err(err)? != REVISION
            || row.get::<_, i64>(10).map_err(err)? != DIM as i64
        {
            return Err("Embedding model revision/dimensions changed. Rebuild the index.".into());
        }
        let mut item = json!({"meeting_id":row.get::<_,String>(0).map_err(err)?,"title":row.get::<_,String>(1).map_err(err)?,"created_at":row.get::<_,i64>(2).map_err(err)?,"start_ms":row.get::<_,i64>(3).map_err(err)?,"end_ms":row.get::<_,i64>(4).map_err(err)?,"text":row.get::<_,String>(5).map_err(err)?,"modality":row.get::<_,String>(6).map_err(err)?,"source":row.get::<_,String>(8).map_err(err)?});
        if !matches(&item, filter) {
            continue;
        }
        let v = vector(&row.get::<_, Vec<u8>>(7).map_err(err)?)?;
        item["cosine"] = json!(q.iter().zip(v).map(|(a, b)| a * b).sum::<f32>());
        results.push(item);
        if results.len() > 160 {
            results.sort_by(|a, b| {
                b["cosine"]
                    .as_f64()
                    .partial_cmp(&a["cosine"].as_f64())
                    .unwrap()
            });
            results.truncate(80);
        }
    }
    results.sort_by(|a, b| {
        b["cosine"]
            .as_f64()
            .partial_cmp(&a["cosine"].as_f64())
            .unwrap()
    });
    Ok(dedup(results, 40))
}
pub fn dedup(items: Vec<Value>, limit: usize) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    for v in items {
        if !out.iter().any(|x| {
            x["meeting_id"] == v["meeting_id"]
                && x["modality"] == v["modality"]
                && (x["start_ms"].as_i64().unwrap_or(0) - v["start_ms"].as_i64().unwrap_or(0)).abs()
                    < 15000
        }) {
            out.push(v);
            if out.len() == limit {
                break;
            }
        }
    }
    out
}
pub fn combined(exact: Vec<Value>, semantic: Vec<Value>) -> Vec<Value> {
    let mut scores: HashMap<String, (f64, Value)> = HashMap::new();
    for list in [exact, semantic] {
        for (i, item) in list.into_iter().enumerate() {
            let key = format!(
                "{}:{}:{}",
                item["meeting_id"], item["start_ms"], item["modality"]
            );
            let entry = scores.entry(key).or_insert((0., item));
            entry.0 += 1. / (60. + i as f64 + 1.);
        }
    }
    let mut values = scores.into_values().collect::<Vec<_>>();
    values.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    dedup(
        values
            .into_iter()
            .map(|(score, mut item)| {
                item["fusion_rank"] = json!(score);
                item
            })
            .collect(),
        40,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_and_roundtrip() {
        assert!(normalized(&[1., 2.]).is_err());
        assert!(normalized(&vec![0.; DIM]).is_err());
        assert!(normalized(&vec![f32::NAN; DIM]).is_err());
        let a = normalized(&vec![1.; DIM]).unwrap();
        assert!((a.iter().map(|x| x * x).sum::<f32>() - 1.).abs() < 0.001);
        assert_eq!(vector(&bytes(&a)).unwrap().len(), DIM);
    }
    #[test]
    fn dedup_keeps_modalities_and_distant_hits() {
        let items = vec![
            json!({"meeting_id":"a","start_ms":0,"modality":"audio"}),
            json!({"meeting_id":"a","start_ms":2000,"modality":"audio"}),
            json!({"meeting_id":"a","start_ms":0,"modality":"text"}),
            json!({"meeting_id":"a","start_ms":30000,"modality":"audio"}),
        ];
        assert_eq!(dedup(items, 40).len(), 3);
    }
}
