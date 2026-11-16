//! Source fetchers and pure parsers. Parsers are unit-tested against the response shapes
//! observed on 2026-11-16 (docs/devlog); fetchers only do HTTP.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::time::{Duration, Instant};

use super::{DigestItem, DigestSettings, SourceStatus};
use crate::compact::truncate_chars;
use crate::model::now_ms;

pub const ALL: &[&str] = &["hn", "github", "lobsters", "reddit", "devto", "hf"];
/// Generic client name — no personal details in any request.
pub const USER_AGENT: &str = "Agora-Digest/0.2 (local daily tech digest)";
const SUMMARY_CHARS: usize = 300;

pub fn label(source: &str) -> &'static str {
    match source {
        "hn" => "Hacker News",
        "github" => "GitHub Trending",
        "lobsters" => "Lobsters",
        "reddit" => "Reddit",
        "devto" => "dev.to",
        "hf" => "Hugging Face Papers",
        _ => "其他",
    }
}

// ------------------------------------------------------------------ text helpers

/// Decode the HTML/XML entities that appear in these feeds.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let ent = &tail[1..end];
        let decoded = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Drop tags and collapse whitespace.
pub fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean(s: &str) -> String {
    strip_tags(&unescape(s))
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)? + i;
    Some(&s[i..j])
}

fn item(source: &str, title: &str, url: &str) -> DigestItem {
    DigestItem { source: source.into(), source_label: label(source).into(), title: title.trim().into(), url: url.trim().into(), ..Default::default() }
}

fn u(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(Value::as_u64)
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string).filter(|x| !x.trim().is_empty())
}

// ------------------------------------------------------------------ parsers

/// Hacker News via the Algolia search API (`hits`).
pub fn parse_hn(json: &str) -> Result<Vec<DigestItem>> {
    let v: Value = serde_json::from_str(json).context("HN 回應不是 JSON")?;
    let hits = v.get("hits").and_then(Value::as_array).ok_or_else(|| anyhow!("HN 回應缺少 hits"))?;
    let mut out: Vec<DigestItem> = hits
        .iter()
        .filter_map(|h| {
            let title = s(h, "title")?;
            let id = s(h, "objectID")?;
            let discussion = format!("https://news.ycombinator.com/item?id={id}");
            let mut i = item("hn", &title, &s(h, "url").unwrap_or_else(|| discussion.clone()));
            i.discussion_url = Some(discussion);
            i.score = u(h, "points");
            i.comments = u(h, "num_comments");
            i.author = s(h, "author");
            i.summary = s(h, "story_text").map(|t| truncate_chars(&clean(&t), SUMMARY_CHARS));
            Some(i)
        })
        .collect();
    out.sort_by(|a, b| b.score.cmp(&a.score));
    Ok(out)
}

/// Lobsters `hottest.json`.
pub fn parse_lobsters(json: &str) -> Result<Vec<DigestItem>> {
    let v: Value = serde_json::from_str(json).context("Lobsters 回應不是 JSON")?;
    let arr = v.as_array().ok_or_else(|| anyhow!("Lobsters 回應不是陣列"))?;
    Ok(arr
        .iter()
        .filter_map(|h| {
            let title = s(h, "title")?;
            let comments = s(h, "comments_url");
            let mut i = item("lobsters", &title, &s(h, "url").or_else(|| comments.clone())?);
            i.discussion_url = comments;
            i.score = u(h, "score");
            i.comments = u(h, "comment_count");
            i.tags = h.get("tags").and_then(Value::as_array).map(|t| t.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
            i.summary = s(h, "description_plain").map(|t| truncate_chars(t.trim(), SUMMARY_CHARS));
            Some(i)
        })
        .collect())
}

/// Reddit `/r/<sub>/top/.rss?t=day` (Atom). No scores in the feed; order = rank.
pub fn parse_reddit_rss(xml: &str, sub: &str) -> Vec<DigestItem> {
    xml.split("<entry>")
        .skip(1)
        .filter_map(|e| {
            let title = unescape(between(e, "<title>", "</title>")?);
            let discussion = between(e, "<link href=\"", "\"")?.to_string();
            let content = unescape(between(e, "<content type=\"html\">", "</content>").unwrap_or(""));
            // Link posts carry `<a href="EXTERNAL">[link]</a>`; self posts point back to reddit.
            let external = content
                .split("<a href=\"")
                .skip(1)
                .filter_map(|p| p.split_once("\">[link]</a>").map(|(href, _)| href.to_string()))
                .next()
                .filter(|h| !h.contains("reddit.com/r/") && !h.contains("redd.it"));
            let mut i = item("reddit", &title, external.as_deref().unwrap_or(&discussion));
            i.source_label = format!("r/{sub}");
            i.discussion_url = Some(discussion);
            i.author = between(e, "<name>", "</name>").map(|a| a.trim_start_matches("/u/").to_string());
            i.tags = vec![sub.to_string()];
            Some(i)
        })
        .collect()
}

/// `github.com/trending` HTML: one `<article class="Box-row">` per repository.
pub fn parse_github_trending(html: &str) -> Vec<DigestItem> {
    html.split("<article class=\"Box-row\"")
        .skip(1)
        .filter_map(|a| {
            let a = a.split("</article>").next().unwrap_or(a);
            let h2 = between(a, "<h2", "</h2>")?;
            let href = h2.split("href=\"/").nth(1)?.split('"').next()?;
            let repo = href.trim_matches('/');
            if repo.split('/').count() != 2 {
                return None;
            }
            let mut i = item("github", repo, &format!("https://github.com/{repo}"));
            i.summary = a
                .split("<p class=\"col-9")
                .nth(1)
                .and_then(|p| p.split_once('>'))
                .and_then(|(_, rest)| rest.split("</p>").next())
                .map(|d| truncate_chars(&clean(d), SUMMARY_CHARS))
                .filter(|d| !d.is_empty());
            i.language = between(a, "itemprop=\"programmingLanguage\">", "<").map(|l| l.trim().to_string());
            let flat = clean(a);
            i.stars_today = flat
                .split(" stars today")
                .next()
                .filter(|_| flat.contains(" stars today"))
                .and_then(|pre| pre.rsplit(' ').next())
                .and_then(|n| n.replace(',', "").parse().ok());
            i.score = a
                .split(&format!("href=\"/{repo}/stargazers\""))
                .nth(1)
                .and_then(|x| x.split_once('>'))
                .and_then(|(_, x)| x.split("</a>").next())
                .map(clean)
                .and_then(|n| n.replace(',', "").trim().parse().ok());
            Some(i)
        })
        .collect()
}

/// GitHub search API fallback (new repositories by stars).
pub fn parse_github_search(json: &str) -> Result<Vec<DigestItem>> {
    let v: Value = serde_json::from_str(json).context("GitHub 回應不是 JSON")?;
    let arr = v.get("items").and_then(Value::as_array).ok_or_else(|| anyhow!("GitHub 回應缺少 items"))?;
    Ok(arr
        .iter()
        .filter_map(|r| {
            let name = s(r, "full_name")?;
            let mut i = item("github", &name, &s(r, "html_url")?);
            i.summary = s(r, "description").map(|d| truncate_chars(&d, SUMMARY_CHARS));
            i.language = s(r, "language");
            i.score = u(r, "stargazers_count");
            i.tags = r.get("topics").and_then(Value::as_array).map(|t| t.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
            Some(i)
        })
        .collect())
}

/// dev.to `/api/articles?top=1`.
pub fn parse_devto(json: &str) -> Result<Vec<DigestItem>> {
    let v: Value = serde_json::from_str(json).context("dev.to 回應不是 JSON")?;
    let arr = v.as_array().ok_or_else(|| anyhow!("dev.to 回應不是陣列"))?;
    let mut out: Vec<DigestItem> = arr
        .iter()
        .filter_map(|a| {
            let mut i = item("devto", &s(a, "title")?, &s(a, "url")?);
            i.score = u(a, "public_reactions_count").or_else(|| u(a, "positive_reactions_count"));
            i.comments = u(a, "comments_count");
            i.summary = s(a, "description").map(|d| truncate_chars(&d, SUMMARY_CHARS));
            i.tags = match a.get("tag_list") {
                Some(Value::Array(t)) => t.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                Some(Value::String(t)) => t.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
                _ => vec![],
            };
            i.author = a.pointer("/user/username").and_then(Value::as_str).map(str::to_string);
            Some(i)
        })
        .collect();
    out.sort_by(|a, b| b.score.cmp(&a.score));
    Ok(out)
}

/// Hugging Face `/api/daily_papers`.
pub fn parse_hf(json: &str) -> Result<Vec<DigestItem>> {
    let v: Value = serde_json::from_str(json).context("Hugging Face 回應不是 JSON")?;
    let arr = v.as_array().ok_or_else(|| anyhow!("Hugging Face 回應不是陣列"))?;
    let mut out: Vec<DigestItem> = arr
        .iter()
        .filter_map(|e| {
            let p = e.get("paper")?;
            let id = s(p, "id")?;
            let title = s(p, "title").or_else(|| s(e, "title"))?;
            let mut i = item("hf", &title.split_whitespace().collect::<Vec<_>>().join(" "), &format!("https://huggingface.co/papers/{id}"));
            i.score = u(p, "upvotes");
            i.comments = u(e, "numComments");
            i.summary = s(p, "ai_summary").or_else(|| s(p, "summary")).map(|d| truncate_chars(&d.split_whitespace().collect::<Vec<_>>().join(" "), SUMMARY_CHARS));
            i.tags = p.get("ai_keywords").and_then(Value::as_array).map(|t| t.iter().take(6).filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
            Some(i)
        })
        .collect();
    out.sort_by(|a, b| b.score.cmp(&a.score));
    Ok(out)
}

// ------------------------------------------------------------------ fetching

/// Fetch with the system `curl` (bundled with Windows 10+, macOS and most Linux). Used as a
/// fallback for hosts whose bot filter rejects our TLS client but not curl (Reddit: 403 for
/// reqwest/rustls, 200 for curl with the same User-Agent — docs/lessons/0006).
pub async fn curl_text(url: &str) -> Result<String> {
    let exe = crate::provider::resolve::find_in_path(&["curl.exe", "curl"]).ok_or_else(|| anyhow!("找不到 curl"))?;
    let mut c = tokio::process::Command::new(exe);
    c.args(["-s", "-f", "-L", "--max-time", "25", "-A", USER_AGENT, url])
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    crate::provider::proc::no_window(&mut c);
    let out = tokio::time::timeout(Duration::from_secs(30), c.output()).await.map_err(|_| anyhow!("curl 逾時"))??;
    if !out.status.success() {
        bail!("curl 失敗（exit {:?}）", out.status.code());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub struct Fetcher {
    client: reqwest::Client,
}

impl Default for Fetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetcher {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(25))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("http client");
        Fetcher { client }
    }

    async fn text(&self, url: &str) -> Result<String> {
        let r = self.client.get(url).send().await.with_context(|| format!("連線失敗：{url}"))?;
        let status = r.status();
        if !status.is_success() {
            bail!("HTTP {status}（{url}）");
        }
        Ok(r.text().await?)
    }

    pub async fn fetch(&self, source: &str, settings: &DigestSettings) -> Result<Vec<DigestItem>> {
        match source {
            "hn" => {
                let since = now_ms() / 1000 - 86_400;
                parse_hn(&self.text(&format!("https://hn.algolia.com/api/v1/search?tags=story&numericFilters=created_at_i%3E{since},points%3E30&hitsPerPage=40")).await?)
            }
            "lobsters" => parse_lobsters(&self.text("https://lobste.rs/hottest.json").await?),
            "devto" => parse_devto(&self.text("https://dev.to/api/articles?top=1&per_page=20").await?),
            "hf" => parse_hf(&self.text("https://huggingface.co/api/daily_papers?limit=25").await?),
            "github" => {
                let trending = self.text("https://github.com/trending?since=daily").await.map(|h| parse_github_trending(&h));
                match trending {
                    Ok(v) if !v.is_empty() => Ok(v),
                    _ => {
                        // Fallback: repositories created in the last week, by stars.
                        let week = crate::engine::format_date(now_ms().saturating_sub(7 * 86_400_000));
                        parse_github_search(&self.text(&format!("https://api.github.com/search/repositories?q=created:%3E{week}&sort=stars&order=desc&per_page=20")).await?)
                    }
                }
            }
            "reddit" => {
                let mut all = Vec::new();
                let mut errors = Vec::new();
                // Best effort: Reddit answers anonymous clients with 403/429 inconsistently
                // (docs/lessons/0006). Space requests out and retry once through curl.
                for (k, sub) in settings.subreddits.iter().filter(|s| !s.trim().is_empty()).enumerate() {
                    if k > 0 {
                        tokio::time::sleep(Duration::from_millis(3000)).await;
                    }
                    let sub = sub.trim().trim_start_matches("r/");
                    let url = format!("https://www.reddit.com/r/{sub}/top/.rss?t=day&limit=8");
                    let body = match self.text(&url).await {
                        Ok(x) => Ok(x),
                        Err(e) if format!("{e}").contains("403") || format!("{e}").contains("429") => {
                            curl_text(&url).await.map_err(|c| anyhow!("{e}；curl 重試：{c}"))
                        }
                        Err(e) => Err(e),
                    };
                    match body {
                        Ok(x) => all.extend(parse_reddit_rss(&x, sub)),
                        Err(e) => errors.push(format!("r/{sub}: {e}")),
                    }
                }
                if all.is_empty() && !errors.is_empty() {
                    bail!(errors.join("；"));
                }
                Ok(all)
            }
            other => bail!("未知的來源：{other}"),
        }
    }
}

/// Fetch every enabled source concurrently (each with its own error) and return items grouped
/// by source in each source's rank order.
pub async fn collect(settings: &DigestSettings) -> (Vec<DigestItem>, Vec<SourceStatus>) {
    let fetcher = std::sync::Arc::new(Fetcher::new());
    let mut set = tokio::task::JoinSet::new();
    for (order, src) in settings.sources.iter().filter(|s| ALL.contains(&s.as_str())).enumerate() {
        let f = fetcher.clone();
        let src = src.clone();
        let st = settings.clone();
        set.spawn(async move {
            let t0 = Instant::now();
            let r = f.fetch(&src, &st).await;
            (order, src, r, t0.elapsed().as_millis() as u64)
        });
    }
    let mut results = Vec::new();
    while let Some(Ok(r)) = set.join_next().await {
        results.push(r);
    }
    results.sort_by_key(|r| r.0);
    let mut items = Vec::new();
    let mut statuses = Vec::new();
    for (_, src, r, ms) in results {
        match r {
            Ok(v) => {
                statuses.push(SourceStatus { source: src.clone(), label: label(&src).into(), ok: true, count: v.len(), error: None, ms });
                items.extend(v);
            }
            Err(e) => statuses.push(SourceStatus { source: src.clone(), label: label(&src).into(), ok: false, count: 0, error: Some(format!("{e:#}")), ms }),
        }
    }
    (items, statuses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_and_strip() {
        assert_eq!(unescape("a &amp; b &lt;c&gt; &#39;d&#39; &#x4E2D;"), "a & b <c> 'd' 中");
        assert_eq!(strip_tags("<p>Hello <b>world</b></p>"), "Hello world");
        assert_eq!(unescape("AT&T"), "AT&T");
    }

    #[test]
    fn hn_uses_discussion_when_no_url_and_sorts_by_points() {
        let j = r#"{"hits":[
          {"title":"Ask HN: X?","objectID":"1","points":50,"num_comments":10,"author":"a","url":null,"story_text":"<p>Body &amp; more</p>"},
          {"title":"Big launch","objectID":"2","points":900,"num_comments":300,"author":"b","url":"https://launch.dev"}]}"#;
        let v = parse_hn(j).unwrap();
        assert_eq!(v[0].title, "Big launch");
        assert_eq!(v[1].url, "https://news.ycombinator.com/item?id=1");
        assert_eq!(v[1].summary.as_deref(), Some("Body & more"));
    }

    #[test]
    fn lobsters_and_devto_and_hf() {
        let l = r#"[{"title":"T","url":"","comments_url":"https://lobste.rs/s/x","score":40,"comment_count":5,"tags":["rust"],"description_plain":"d"}]"#;
        let v = parse_lobsters(l).unwrap();
        assert_eq!(v[0].url, "https://lobste.rs/s/x");
        assert_eq!(v[0].tags, vec!["rust"]);

        let d = r#"[{"title":"A","url":"https://dev.to/a","public_reactions_count":12,"comments_count":3,"tag_list":["ai","rust"],"description":"x","user":{"username":"u"}},
                   {"title":"B","url":"https://dev.to/b","public_reactions_count":99,"comments_count":1,"tag_list":"web, css","description":"y","user":{"username":"v"}}]"#;
        let v = parse_devto(d).unwrap();
        assert_eq!(v[0].title, "B");
        assert_eq!(v[0].tags, vec!["web", "css"]);

        let h = r#"[{"numComments":2,"paper":{"id":"2611.01","title":"Long\n  Title","upvotes":77,"ai_summary":"Summary.","ai_keywords":["agents"]}}]"#;
        let v = parse_hf(h).unwrap();
        assert_eq!(v[0].title, "Long Title");
        assert_eq!(v[0].url, "https://huggingface.co/papers/2611.01");
        assert_eq!(v[0].score, Some(77));
    }

    #[test]
    fn reddit_rss_extracts_external_link_and_discussion() {
        let xml = r#"<feed><entry><author><name>/u/someone</name></author><content type="html">&amp;#32; submitted by &lt;a href=&quot;https://www.reddit.com/user/someone&quot;&gt; /u/someone &lt;/a&gt; &lt;br/&gt; &lt;span&gt;&lt;a href=&quot;https://slack.engineering/cells/&quot;&gt;[link]&lt;/a&gt;&lt;/span&gt; &lt;span&gt;&lt;a href=&quot;https://www.reddit.com/r/programming/comments/1/x/&quot;&gt;[comments]&lt;/a&gt;&lt;/span&gt;</content><id>t3_1</id><link href="https://www.reddit.com/r/programming/comments/1/x/" /><title>Slack&#39;s Migration to Cells</title></entry></feed>"#;
        let v = parse_reddit_rss(xml, "programming");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].title, "Slack's Migration to Cells");
        assert_eq!(v[0].url, "https://slack.engineering/cells/");
        assert_eq!(v[0].discussion_url.as_deref(), Some("https://www.reddit.com/r/programming/comments/1/x/"));
        assert_eq!(v[0].source_label, "r/programming");
        assert_eq!(v[0].author.as_deref(), Some("someone"));
    }

    /// Network probe, not part of the normal suite: `cargo test -p agora-core -- --ignored reddit_probe --nocapture`
    #[tokio::test]
    #[ignore]
    async fn reddit_probe() {
        let url = "https://www.reddit.com/r/rust/top/.rss?t=day&limit=5";
        let c = reqwest::Client::builder().user_agent(USER_AGENT).build().unwrap();
        for accept in ["*/*", "application/atom+xml,application/xml;q=0.9,*/*;q=0.8"] {
            let r = c.get(url).header("accept", accept).send().await.unwrap();
            println!("reqwest accept={accept} -> {}", r.status());
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        let h1 = reqwest::Client::builder().user_agent(USER_AGENT).http1_only().build().unwrap();
        println!("reqwest http1 -> {}", h1.get(url).send().await.unwrap().status());
        tokio::time::sleep(Duration::from_secs(3)).await;
        println!("curl fallback -> {:?}", curl_text(url).await.map(|s| s.len()));
    }

    #[test]
    fn github_trending_article() {
        let html = r#"<article class="Box-row"> <h2 class="h3 lh-condensed"> <a data-hydro-click="{}" href="/dream-num/univer" data-view-component="true" class="Link"> dream-num / univer </a> </h2>
          <p class="col-9 color-fg-muted my-1 tmp-pr-4"> Build your own AI office &amp; more. </p>
          <span itemprop="programmingLanguage">TypeScript</span>
          <a href="/dream-num/univer/stargazers" class="Link"> <svg></svg> 28,512 </a>
          <span class="d-inline-block float-sm-right"> <svg></svg> 1,041 stars today </span></article>"#;
        let v = parse_github_trending(html);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].title, "dream-num/univer");
        assert_eq!(v[0].summary.as_deref(), Some("Build your own AI office & more."));
        assert_eq!(v[0].language.as_deref(), Some("TypeScript"));
        assert_eq!(v[0].stars_today, Some(1041));
        assert_eq!(v[0].score, Some(28512));
    }
}
