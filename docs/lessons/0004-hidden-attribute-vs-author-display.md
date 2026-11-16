# 0004 — 作者寫的 `display` 會蓋掉 `hidden` 屬性

- **Date**: 2026-11-16
- **Grade**: 已確認

## 發生了什麼

議題頁底部的插話框是 `<form class="interject" hidden>`，議題結束時用 `el.hidden = true` 把它藏起來。但 `.interject { display: flex }` 的優先權比瀏覽器內建的 `[hidden] { display: none }` 高，所以議題結束後，插話框仍然顯示。在已結束的議題送出插話，伺服器會回「議題已結束」。

## 證據

- 截圖：示範模式下，一場已完成的辯論底部仍然顯示插話框（2026-11-16）。
- 修正後再看同一個議題，插話框已隱藏。

## 規則

`web/app.css` 的開頭加上 `[hidden] { display: none !important; }`。之後所有用 `hidden` 切換顯示的元件都可以依賴它，不必逐個元件處理。
