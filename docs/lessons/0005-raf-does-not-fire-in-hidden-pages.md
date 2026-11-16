# 0005 — 頁面在背景時，`requestAnimationFrame` 不會觸發

- **Date**: 2026-11-16
- **Grade**: 候選（現象已確認；「頁面當時在背景」是最可能的原因，但沒有直接觀察到）

## 發生了什麼

議題頁把 SSE 事件合併起來，用 `requestAnimationFrame` 排一次重繪，用 `paintQueued` 旗標避免重複排程。有一次從介面開始工作坊後，後端已經停在「等待核准」（API 確認），頁面卻一直顯示「修訂方案」，也沒有出現核准面板。

用 curl 直接訂閱同一個 SSE，`phase`、`plan`、`status:awaiting_approval` 事件都有正確送出。在頁面加了事件記錄後重做一次，同一個流程就正常了。

## 證據

- 失敗：在 `#/s/s-1a1e0a5aeed4f513f` 上，API 回 `awaiting_approval`，介面顯示「修訂方案」。
- 對照：`curl /api/sessions/{id}/events` 收到 `phase 等待核准` 與 `status awaiting_approval`。
- 重現時（頁面可見）：`evlog` 收到完整事件，核准面板正常出現。

## 規則

- 重繪排程一律用 `nextFrame()`：同時排 `requestAnimationFrame` 和一個 250 ms 的 `setTimeout`，哪個先到就執行哪個（`web/js/session.js`）。
- 任何「只靠 rAF 才會更新」的狀態都要有計時器備援。尤其桌面版最小化、手機切到背景時，正是使用者在等通知的時候。
- `window.__agora.evlog` 保留最近收到的事件類型，下次可以直接查。
