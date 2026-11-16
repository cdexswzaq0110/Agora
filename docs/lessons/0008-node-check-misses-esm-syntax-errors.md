# 0008 — `node --check x.js` 沒抓到 ES module 裡的語法錯誤

- **Date**: 2026-11-16
- **Grade**: 已確認（最小重現）

## 發生了什麼

v0.3 改 `web/js/session.js` 時，一次腳本替換把 `split('\n')` 寫成了字串裡真的換行（未結束的字串常值）。`node --check web/js/session.js` 回傳 0，於是以為沒問題；瀏覽器載入時卻是 `Uncaught SyntaxError: Invalid or unexpected token`，整個前端都沒啟動（`window.__agora` 是 `undefined`）。

## 證據

Node v24.16.0，最小重現檔 `bad.js`：

```js
import { x } from './other.js';
export const lines = (t) => t.split('
');
```

| 指令 | 結束碼 |
|---|---|
| `node --check bad.js` | **0** |
| `node --check bad.mjs`（同內容） | 1，指出第 2 行 |
| `node --check --input-type=module < bad.js` | 1 |

沒有 `package.json` 的 `"type": "module"` 時，`.js` 檔的模組格式要靠偵測；這條路徑在這個 Node 版本上沒有回報語法錯誤。

## 規則

- 前端檢查改用 `node scripts/check-web.mjs`：用 `--input-type=module` 從 stdin 解析每個 module，並檢查具名 import 是否存在、`sw.js` 是否快取了每個 module。
- 最終驗收仍要在瀏覽器載入：console 沒有 error，而且每個 module 都回 200。
- 用 shell 做含反斜線的文字替換時，先確認結果。這次 heredoc 裡的 Python、`python -c`、`perl -e` 三種寫法都把 `\n` 變成真的換行；是哪一層少了一次跳脫沒有釐清（候選：指令傳遞時被解讀了一次）。改用寫檔工具後就正常，所以含反斜線的修改改用寫檔工具。
