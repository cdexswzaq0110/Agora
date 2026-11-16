# 0013 — 「動態效果：開」被三層「降低動態」的限制吃掉

- **Date**: 2026-11-16
- **Grade**: 已確認（瀏覽器量測：修正前 0 個動畫 → 修正後 4 種動畫在跑；切到「關」回到 0）

## 發生了什麼

v0.5 的動態效果（GPT 實作）在設定頁有「自動（跟隨系統）／開／關」三個選項。驗收時內建瀏覽器面板的系統設定是 `prefers-reduced-motion: reduce`，選「開」之後畫面仍然完全不動。

「開」的意思是**你明確選擇要動態**，但這個選擇在三個地方都被系統設定蓋掉：

1. `motion.js`：`motionOn = setting !== 'off' && !reduce.matches`——系統要求降低動態時，「開」和「自動」沒有差別。
2. `app.css` 動態區塊：`@media (prefers-reduced-motion: reduce) { .motion-on * { animation-duration: .001ms !important } }`。
3. `app.css` v0.3 留下的全域規則：`@media (prefers-reduced-motion: reduce) { *, *::before, *::after { animation: none !important } }`。

只修第 1 層時，`<html>` 已經是 `motion-on`，`.motion-noise` 的 `opacity` 也套上了，但 `getComputedStyle(...).animationName` 仍是 `none`——第 3 層的 `!important` 把動畫整個拿掉。

## 證據

- 修正前：設定「開」、重新整理後 1.8 秒，`document.getAnimations().length === 0`；`.motion-noise` 的 `animationName: none`、`animationDuration: 0s`。
- 修正後（`motion-forced` class）：`page-fade-rise`、`split-up`、`status-pulse`、`noise-shift` 都在跑；`setMotionSetting('off')` 之後 `getAnimations()` 回到 0、`#view` 的 `opacity` 是 1、沒有 `.reveal-pending` 殘留。

## 規則

- 能**覆寫系統偏好**的使用者設定，要在每一層尊重那個偏好的地方都檢查一次（JS 判斷、元件 CSS、全域 CSS）。實作上用一個 class（`motion-forced`）代表「你明確選了開」，所有降低動態的 CSS 都寫成 `:root:not(.motion-forced) …`。
- 系統偏好相關的驗收要兩種環境都測：系統要求降低動態時，「自動」要停、「開」要動、「關」要停。只在一般環境測，三個選項看起來都對。
- 其他模組（例如本機擴充的桌寵）判斷動態時看 `<html>` 的 `motion-on`／`motion-off`，不要自己再看一次 `prefers-reduced-motion`；只有 `motion.js` 沒載入（兩個 class 都沒有）時才退回看系統設定。
