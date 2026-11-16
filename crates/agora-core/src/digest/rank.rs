//! De-duplicate, score and select digest items.
//!
//! Score = position within its source (1.0 for #1 → ~0 for last) × source weight
//!       + 0.35 per matched interest keyword (max 2)
//!       + 0.30 per additional source carrying the same link (cross-forum attention).
//! Selection keeps at least `MIN_PER_SOURCE` from every source that returned anything, at most
//! `MAX_PER_SOURCE`, then fills by score.

use std::collections::HashMap;

use super::DigestItem;

pub const MIN_PER_SOURCE: usize = 3;
pub const MAX_PER_SOURCE: usize = 10;

pub fn source_weight(source: &str) -> f64 {
    match source {
        "hn" | "github" => 1.0,
        "lobsters" | "hf" => 0.85,
        s if s.starts_with("reddit") => 0.75,
        "devto" => 0.55,
        _ => 0.6,
    }
}

/// Canonical form of a URL for de-duplication.
pub fn canonical(url: &str) -> String {
    let u = url.trim().to_ascii_lowercase();
    let u = u.split('#').next().unwrap_or("");
    let (base, query) = u.split_once('?').unwrap_or((u, ""));
    let base = base.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.").trim_end_matches('/');
    let kept: Vec<&str> = query.split('&').filter(|kv| !kv.is_empty() && !kv.starts_with("utm_") && !kv.starts_with("ref=")).collect();
    if kept.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", kept.join("&"))
    }
}

pub(crate) fn norm_title(t: &str) -> String {
    t.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

pub fn keyword_hits(item: &DigestItem, keywords: &[String]) -> Vec<String> {
    let hay = format!(
        "{} {} {} {}",
        item.title,
        item.summary.as_deref().unwrap_or(""),
        item.tags.join(" "),
        item.language.as_deref().unwrap_or("")
    )
    .to_lowercase();
    keywords
        .iter()
        .filter(|k| !k.trim().is_empty() && hay.contains(&k.trim().to_lowercase()))
        .cloned()
        .collect()
}

/// `items` must be grouped by source in each source's own rank order (as fetched).
pub fn select(items: Vec<DigestItem>, keywords: &[String], max: usize) -> Vec<DigestItem> {
    // Position score within each source.
    let mut per_source: HashMap<String, usize> = HashMap::new();
    for i in &items {
        *per_source.entry(i.source.clone()).or_default() += 1;
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut scored: Vec<DigestItem> = items
        .into_iter()
        .map(|mut i| {
            let pos = seen.entry(i.source.clone()).or_default();
            let len = per_source[&i.source].max(1) as f64;
            let position = 1.0 - (*pos as f64) / len;
            *pos += 1;
            i.matched_keywords = keyword_hits(&i, keywords);
            i.rank = position * source_weight(&i.source) + 0.35 * (i.matched_keywords.len().min(2) as f64);
            i
        })
        .collect();

    // De-duplicate by canonical URL or normalised title; merge into the best-ranked copy.
    scored.sort_by(|a, b| b.rank.partial_cmp(&a.rank).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<DigestItem> = Vec::new();
    for i in scored {
        let key_u = canonical(&i.url);
        let key_t = norm_title(&i.title);
        if let Some(existing) = out
            .iter_mut()
            .find(|o| canonical(&o.url) == key_u || (!key_t.is_empty() && key_t.len() > 12 && norm_title(&o.title) == key_t))
        {
            if existing.source != i.source && !existing.also_on.contains(&i.source_label) {
                existing.also_on.push(i.source_label.clone());
                existing.rank += 0.3;
            }
            if existing.discussion_url.is_none() {
                existing.discussion_url = i.discussion_url.clone();
            }
            continue;
        }
        out.push(i);
    }
    out.sort_by(|a, b| b.rank.partial_cmp(&a.rank).unwrap_or(std::cmp::Ordering::Equal));

    // Per-source floor, then fill by score under the per-source cap.
    let mut taken: Vec<bool> = vec![false; out.len()];
    let mut count: HashMap<String, usize> = HashMap::new();
    let mut picked = 0usize;
    for (idx, i) in out.iter().enumerate() {
        let c = count.entry(i.source.clone()).or_default();
        if *c < MIN_PER_SOURCE && picked < max {
            *c += 1;
            taken[idx] = true;
            picked += 1;
        }
    }
    for (idx, i) in out.iter().enumerate() {
        if picked >= max {
            break;
        }
        let c = count.entry(i.source.clone()).or_default();
        if !taken[idx] && *c < MAX_PER_SOURCE {
            *c += 1;
            taken[idx] = true;
            picked += 1;
        }
    }
    let mut result: Vec<DigestItem> = out.into_iter().zip(taken).filter(|(_, t)| *t).map(|(i, _)| i).collect();
    result.sort_by(|a, b| b.rank.partial_cmp(&a.rank).unwrap_or(std::cmp::Ordering::Equal));
    for (k, i) in result.iter_mut().enumerate() {
        i.n = k as u32 + 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(source: &str, title: &str, url: &str) -> DigestItem {
        DigestItem { source: source.into(), source_label: source.to_uppercase(), title: title.into(), url: url.into(), ..Default::default() }
    }

    #[test]
    fn canonical_strips_tracking_and_scheme() {
        assert_eq!(canonical("https://www.Example.com/a/?utm_source=hn&id=3#x"), "example.com/a?id=3");
        assert_eq!(canonical("http://example.com/a/"), "example.com/a");
    }

    #[test]
    fn duplicates_merge_and_get_cross_source_boost() {
        let items = vec![
            it("hn", "Rust 2026 roadmap", "https://blog.rust-lang.org/roadmap"),
            it("hn", "Other", "https://o.dev"),
            it("lobsters", "Rust 2026 roadmap", "https://blog.rust-lang.org/roadmap/?utm_source=lobsters"),
        ];
        let out = select(items, &[], 10);
        assert_eq!(out.len(), 2);
        let r = out.iter().find(|i| i.title.starts_with("Rust")).unwrap();
        assert_eq!(r.also_on, vec!["LOBSTERS".to_string()]);
        assert_eq!(out[0].n, 1);
    }

    #[test]
    fn keywords_boost_and_every_source_is_represented() {
        let mut items: Vec<DigestItem> = (0..20).map(|k| it("hn", &format!("HN story {k}"), &format!("https://hn/{k}"))).collect();
        items.push(it("devto", "Building an MCP agent in Rust", "https://dev.to/x"));
        items.push(it("devto", "CSS tips", "https://dev.to/y"));
        let out = select(items, &["mcp".into()], 8);
        assert_eq!(out.len(), 8);
        assert!(out.iter().any(|i| i.source == "devto"), "low-weight source still gets its floor");
        let mcp = out.iter().find(|i| i.title.contains("MCP")).unwrap();
        assert_eq!(mcp.matched_keywords, vec!["mcp".to_string()]);
        assert!(out.iter().filter(|i| i.source == "hn").count() <= MAX_PER_SOURCE);
    }
}
