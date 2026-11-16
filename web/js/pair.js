// Pairing screen for phones. A QR code from the desktop carries the code in the URL fragment
// (`#pair=123456`) — fragments never leave the browser — and pre-fills it here.
import { $, api, esc, toast } from './util.js';

export function pairCodeFromHash() {
  const m = location.hash.match(/pair=(\d{6})/);
  return m ? m[1] : '';
}

export function renderPair(onPaired) {
  const code = pairCodeFromHash();
  const fromQr = Boolean(code);
  const name =/iPhone|iPad/.test(navigator.userAgent) ? 'iPhone' : /Android/.test(navigator.userAgent) ? 'Android 手機' : '瀏覽器';
  $('#view').innerHTML = `<div class="pair-screen"><form class="pair-card" id="pairForm">
    <p class="eyebrow">Pair · 配對這台裝置</p>
    <h1 style="margin:0;font:600 1.5rem/1.3 var(--font-display)">${code ? '確認配對' : '輸入電腦上的配對碼'}</h1>
    <p class="muted" style="margin:0;font-size:.9rem">在電腦打開 Agora →「設定 → 手機連線」（或頁尾的「手機連線」），那裡有 6 位數配對碼與 QR code。</p>
    ${code ? '' : `<ol class="pair-help"><li>用手機相機掃電腦上的 QR code 最快，配對碼會自動帶入。</li><li>不在同一個 Wi-Fi 時：手機要先打開 Tailscale App，並登入和電腦同一個帳號。</li><li>配對碼 10 分鐘內有效；過期就看電腦上的新號碼。</li></ol>`}
    <input class="code" name="code" inputmode="numeric" autocomplete="one-time-code" maxlength="7" required aria-label="配對碼" value="${esc(code)}">
    <label class="field"><span>這台裝置的名稱</span><input type="text" name="name" value="${esc(name)}"></label>
    <button class="btn primary" type="submit">配對</button>
    <p class="pair-error" id="pairError" role="alert" hidden></p>
  </form></div>`;
  $('#pairForm').onsubmit = async (e) => {
    e.preventDefault();
    const error = $('#pairError');
    error.hidden = true;
    try {
      await api('/api/pair', { method: 'POST', body: { code: e.target.code.value.replace(/\s/g, ''), name: e.target.name.value } });
      toast('配對完成');
      history.replaceState(null, '', location.pathname + location.search + '#/');
      await onPaired();
    } catch (err) {
      // Server says 「配對碼已過期」 for an expired code (auth.rs Pairing::try_pair).
      const message = /失效|過期/.test(err.message)
        ? '這個配對碼已過期。請回到電腦的「手機連線」掃描新 QR code。'
        : `配對失敗：${err.message}`;
      error.textContent = message;
      error.hidden = false;
      if (!fromQr) toast(message, true);
    }
  };
}
