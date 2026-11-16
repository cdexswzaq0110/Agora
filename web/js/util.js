// Shared helpers. Every dynamic value goes through esc() before it touches innerHTML.

export const $ = (sel, root = document) => root.querySelector(sel);
export const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));
const ESC = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
export const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ESC[c]);
export const pad2 = (n) => String(n).padStart(2, '0');

/** Run `fn` on the next frame — or after 250 ms when requestAnimationFrame does not fire
 * (hidden or throttled pages, lessons/0005). Every coalesced repaint goes through this. */
export function nextFrame(fn) {
  let done = false;
  const run = () => { if (!done) { done = true; fn(); } };
  requestAnimationFrame(run);
  setTimeout(run, 250);
}

export const store = {
  get(k, d) { try { const v = localStorage.getItem('agora:' + k); return v == null ? d : JSON.parse(v); } catch { return d; } },
  set(k, v) { try { localStorage.setItem('agora:' + k, JSON.stringify(v)); } catch { /* private mode */ } },
};

export async function api(path, { method = 'GET', body } = {}) {
  const res = await fetch(path, {
    method,
    credentials: 'same-origin',
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : {},
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  let data = null;
  try { data = await res.json(); } catch { /* empty */ }
  if (!res.ok) {
    const err = new Error((data && data.error) || `${res.status} ${res.statusText}`);
    err.status = res.status;
    throw err;
  }
  return data;
}

let toastTimer;
export function toast(msg, bad = false) {
  let el = $('#toast');
  if (!el) { el = document.createElement('div'); el.id = 'toast'; el.setAttribute('role', 'status'); document.body.appendChild(el); }
  el.className = 'toast' + (bad ? ' bad' : '');
  el.textContent = msg;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { el.hidden = true; }, bad ? 7000 : 3200);
}

export function rel(ms) {
  if (!ms) return '—';
  const d = (Date.now() - ms) / 1000;
  if (d < 45) return '剛剛';
  if (d < 3600) return `${Math.floor(d / 60)} 分鐘前`;
  if (d < 86400) return `${Math.floor(d / 3600)} 小時前`;
  return `${Math.floor(d / 86400)} 天前`;
}
export function until(sec) {
  if (!sec) return '';
  const d = sec - Date.now() / 1000;
  if (d <= 0) return '已重置';
  const h = Math.floor(d / 3600), m = Math.floor((d % 3600) / 60);
  if (h >= 48) return `${Math.floor(h / 24)} 天後重置`;
  if (h > 0) return `${h} 小時 ${m} 分後重置`;
  return `${Math.max(1, m)} 分鐘後重置`;
}
export function dur(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} 秒`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} 分 ${s % 60} 秒`;
  return `${Math.floor(m / 60)} 小時 ${m % 60} 分`;
}
export function stamp(ms) {
  const d = new Date(ms);
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}
export const num = (n) => (n == null ? '—' : Number(n).toLocaleString('zh-TW'));
export function compact(n) {
  if (n == null) return '—';
  const a = Math.abs(n);
  if (a >= 1e6) return `${(n / 1e6).toFixed(a >= 1e7 ? 0 : 1)}M`;
  if (a >= 1e4) return `${(n / 1e3).toFixed(0)}K`;
  return num(n);
}

export async function copyText(text) {
  try { await navigator.clipboard.writeText(text); return true; } catch { /* insecure context */ }
  const ta = document.createElement('textarea');
  ta.value = text; ta.style.position = 'fixed'; ta.style.opacity = '0';
  document.body.appendChild(ta); ta.select();
  let ok = false;
  try { ok = document.execCommand('copy'); } catch { ok = false; }
  ta.remove();
  return ok;
}

export function localDate(d = new Date()) {
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}
