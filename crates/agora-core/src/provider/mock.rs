//! Deterministic offline provider. Used by the test suite and by demo mode
//! (`AGORA_DEMO=1` or the "示範模式" setting) so the whole UI can be exercised without
//! spending any quota.
//!
//! Prompt markers for tests: `[mock:fail]` fails the turn, `[mock:slow]` makes it slow
//! (for cancellation), `[mock:split]` makes the GPT judge disagree.

use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::{Health, ModelOption, Provider, ProviderEvent, Sink, TurnOutput, TurnRequest};
use crate::model::{now_ms, Family, TokenUsage, ToolNote};
use crate::quota::{QuotaSnapshot, QuotaStatus, QuotaWindow};

pub struct MockProvider {
    id: String,
    family: Family,
    pub delay: Duration,
    calls: AtomicU64,
}

impl MockProvider {
    pub fn new(id: &str, family: Family) -> Self {
        MockProvider { id: id.into(), family, delay: Duration::from_millis(60), calls: AtomicU64::new(0) }
    }

    pub fn with_delay(mut self, d: Duration) -> Self {
        self.delay = d;
        self
    }

    fn role_of(prompt: &str) -> &'static str {
        if prompt.contains("每日技術情報編輯") {
            return "情報編輯";
        }
        for r in ["審查者", "執行者", "與談人", "總結者", "正方", "反方", "裁判", "提案者", "評論者", "探測", "單人"] {
            if prompt.contains(&format!("本輪身分：**{r}**")) {
                return r;
            }
        }
        "單人"
    }

    fn answer(&self, prompt: &str) -> String {
        let who = format!("模擬 {}", self.family.label());
        let topic: String = prompt
            .split("## 議題\n")
            .nth(1)
            .and_then(|s| s.lines().next())
            .unwrap_or("")
            .chars()
            .take(40)
            .collect();
        let interjected = prompt.contains("## 使用者插話");
        let mut s = match Self::role_of(prompt) {
            "情報編輯" => format!(
                "（{who}）\n\n## 今日三大重點\n- 第一則項目受到最多關注 [1]\n- 跨來源的話題 [2]【推論】\n- 新工具發布 [3]\n## 技術趨勢觀察\n- AI agent 工具鏈持續升溫 [1][2]\n## GitHub 熱門專案\n- 範例專案 — 示範用 [3]\n## 論壇熱議\n- 討論串 [2]\n## 建議深入閱讀\n- [1]\n## 名詞小辭典\n- MCP：讓模型呼叫外部工具的協定 [1]"
            ),
            "提案者" => format!(
                "（{who}）\n\n## 我理解的目標\n完成「{topic}」。\n## 方案\n用最小改動完成，{}。\n## 實作步驟\n1. 新增模組\n2. 補測試\n## 會動到的檔案\n- src/feature.rs\n## 風險與對策\n- 相容性：保留舊介面\n## 驗證方式\n- 執行測試\n## 成本估計\n小",
                if self.family == Family::Claude { "偏重可讀性" } else { "偏重效能" }
            ),
            "評論者" if prompt.contains("紅隊") => format!("（{who}）\n\n- 【中】驗收標準第 2 條無法自動驗證，建議改成可執行的測試\n- 【低】沒有說明失敗時如何回復"),
            "評論者" => format!(
                "（{who}）\n\n| 提案 | 正確性 | 簡潔度 | 風險 | 成本 | 可測試性 |\n|---|---|---|---|---|---|\n| Claude | 4 | 5 | 4 | 4 | 4 |\n| GPT | 4 | 4 | 4 | 3 | 5 |\n\n建議組合：Claude 的結構加上 GPT 的測試策略。"
            ),
            "總結者" if prompt.contains("可以直接交給工程師實作") || prompt.contains("修訂最終方案") => format!(
                "（{who}）\n\n## 提案評分\n| 提案 | 總分 |\n|---|---|\n| Claude | 21 |\n| GPT | 20 |\n## 最終方案\n### 目標\n完成「{topic}」。\n### 採用的做法與理由\n採用 Claude 的結構與 GPT 的測試策略。\n### 實作步驟\n1. 新增模組\n2. 補上測試\n### 會動到的檔案\n- src/feature.rs\n### 風險與對策\n- 保留舊介面\n### 驗收標準\n1. 新功能有測試且通過\n2. 舊測試全部通過\n### 建議的驗證指令\n`cargo test`\n### 不做的事\n- 不改公開 API\n整體證據等級：推論"
            ),
            "執行者" if prompt.contains("要求修訂") => format!(
                "（{who}）已依審查意見修正。\n\n## 修訂摘要\n- 【高】補上空輸入處理\n- 【低】命名已統一"
            ),
            "執行者" => format!(
                "（{who}）完成「{topic}」的初版。\n\n## 交付摘要\n- 新增核心流程\n- 修改 1 個檔案\n- 驗證方式：執行測試"
            ),
            "審查者" if prompt.contains("修訂摘要") => {
                format!("（{who}）修訂後的版本已處理先前問題。\n\n- 【低】可再補文件\n\nVERDICT: APPROVE")
            }
            "審查者" => format!("（{who}）發現問題：\n\n- 【高】沒有處理空輸入\n- 【低】命名不一致\n\nVERDICT: REVISE"),
            "與談人" => format!(
                "（{who}）\n\n## 立場\n針對「{topic}」，我建議先做最小可行版本。\n## 理由\n- 風險低\n## 我不確定的地方\n- 使用量規模【未知】"
            ),
            "總結者" => format!(
                "（{who}）\n\n## 共識\n- 先做最小可行版本\n## 分歧\n- 擴充時機\n## 建議做法\n先上線最小版本，再依使用量擴充。\n## 待驗證事項\n- 實際使用量\n整體證據等級：推論"
            ),
            "正方" => format!("（{who}）\n\n## 主張\n採用方案 A。\n## 論證\n- 成本低【推論】"),
            "反方" => format!("（{who}）\n\n## 質疑\n方案 A 擴充性不足。\n## 替代方案\n方案 B。"),
            "裁判" => {
                let winner = if prompt.contains("[mock:split]") && self.family == Family::Gpt { "反方" } else { "正方" };
                format!(
                    "（{who}）\n\n## 裁決\n{winner}論證較完整。\n## 建議做法\n先採用 A，保留換成 B 的介面。\n## 待驗證事項\n- 負載測試\n勝方：{winner}\n整體證據等級：推論"
                )
            }
            "探測" => "OK".into(),
            _ => format!("（{who}）針對「{topic}」：建議先釐清目標，再分階段實作。"),
        };
        if interjected {
            s.push_str("\n\n（已納入使用者插話）");
        }
        if prompt.contains("[mock:echo-headers]") {
            let headers: Vec<&str> = prompt.lines().filter(|l| l.starts_with("### 第")).collect();
            s.push_str(&format!("\n\n看到的發言標題：{}", headers.join(" / ")));
        }
        s
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn family(&self) -> Family {
        self.family
    }
    fn display_name(&self) -> String {
        format!("模擬 {}（離線示範）", self.family.label())
    }
    fn is_mock(&self) -> bool {
        true
    }
    fn models(&self) -> Vec<ModelOption> {
        vec![ModelOption { id: String::new(), label: "模擬模型".into() }]
    }

    async fn health(&self) -> Health {
        Health {
            ok: true,
            summary: "離線示範模式（不呼叫任何 CLI、不使用額度）".into(),
            fix: None,
            version: Some(crate::VERSION.into()),
            account: Some("demo".into()),
            executable: None,
            checked_at: now_ms(),
        }
    }

    async fn poll_quota(&self) -> Option<QuotaSnapshot> {
        Some(self.quota())
    }

    async fn run(&self, req: TurnRequest, sink: Sink) -> TurnOutput {
        let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if req.prompt.contains("[mock:fail]") {
            return TurnOutput::fail("模擬失敗");
        }
        let delay = if req.prompt.contains("[mock:slow]") { self.delay * 50 } else { self.delay };
        let text = self.answer(&req.prompt);
        // Lets tests exercise workspace isolation end to end: a "builder" that really edits.
        if req.access == crate::model::Access::Write && req.prompt.contains("[mock:write]") && req.prompt.contains("本輪身分：**執行者**") {
            let _ = std::fs::write(req.cwd.join("agora-mock.txt"), format!("written by {} ({n})\n", self.id));
        }
        let _ = sink.send(ProviderEvent::Tool(ToolNote { name: "Read".into(), detail: "README.md".into() }));
        let chars: Vec<char> = text.chars().collect();
        let step = (chars.len() / 4).max(1);
        for chunk in chars.chunks(step) {
            tokio::select! {
                _ = req.cancel.cancelled() => {
                    return TurnOutput { error: Some("已取消".into()), ..Default::default() };
                }
                _ = tokio::time::sleep(delay) => {}
            }
            let _ = sink.send(ProviderEvent::Delta(chunk.iter().collect()));
        }
        let _ = sink.send(ProviderEvent::Quota(self.quota_after(n)));
        TurnOutput {
            ok: true,
            text,
            usage: Some(TokenUsage {
                input_tokens: (req.prompt.chars().count() / 2) as u64,
                output_tokens: (chars.len() / 2) as u64,
                cached_input_tokens: 0,
            }),
            ..Default::default()
        }
    }
}

impl MockProvider {
    fn quota(&self) -> QuotaSnapshot {
        self.quota_after(self.calls.load(Ordering::SeqCst))
    }

    fn quota_after(&self, calls: u64) -> QuotaSnapshot {
        let now = now_ms();
        let five = (8 + calls * 6).min(100) as f64;
        let week = (20 + calls * 2).min(100) as f64;
        QuotaSnapshot {
            provider: self.id.clone(),
            family: self.family,
            plan: Some("demo".into()),
            status: if five >= 80.0 { QuotaStatus::Warning } else { QuotaStatus::Ok },
            windows: vec![
                QuotaWindow { key: "five_hour".into(), label: "5 小時".into(), used_percent: five, resets_at: Some(now / 1000 + 3 * 3600), window_minutes: Some(300) },
                QuotaWindow { key: "weekly".into(), label: "每週".into(), used_percent: week, resets_at: Some(now / 1000 + 4 * 86400), window_minutes: Some(10080) },
            ],
            source: "模擬資料".into(),
            observed_at: now,
        }
    }
}
