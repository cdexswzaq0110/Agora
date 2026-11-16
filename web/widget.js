/* Always-on-top usage widget: one HP bar per provider, live over SSE. */
'use strict';
const ESC = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ESC[c]);
let providers = [];
const quotas = {};

function remaining(q) {
  if (!q || !q.windows || !q.windows.length) return null;
  return Math.max(0, 100 - Math.max(...q.windows.map((w) => w.used_percent)));
}
function hp(rem) {
  const cls = rem == null ? 'x' : rem > 60 ? 'g' : rem > 40 ? 'y' : rem > 20 ? 'o' : 'r';
  const on = rem == null ? 0 : Math.ceil(rem / 20);
  return `<span class="hp ${cls}">${[0, 1, 2, 3, 4].map((i) => `<i class="${i < on ? 'on' : ''}"></i>`).join('')}</span>`;
}
function tightest(q) {
  if (!q || !q.windows || !q.windows.length) return null;
  return q.windows.reduce((a, b) => (b.used_percent > a.used_percent ? b : a));
}
function until(sec) {
  if (!sec) return '';
  const d = sec - Date.now() / 1000;
  if (d <= 0) return '已重置';
  const h = Math.floor(d / 3600), m = Math.floor((d % 3600) / 60);
  if (h >= 48) return `${Math.floor(h / 24)} 天後重置`;
  return h > 0 ? `${h} 小時 ${m} 分後重置` : `${Math.max(1, m)} 分後重置`;
}
function paint() {
  const list = providers.filter((p) => !p.mock || quotas[p.id]);
  document.getElementById('rows').innerHTML = list.length ? list.map((p) => {
    const q = quotas[p.id] || p.quota;
    const rem = remaining(q);
    const w = tightest(q);
    return `<div class="w-row"><b class="fam-${esc(p.family)}">${p.mock ? '模擬 ' : ''}${p.family === 'claude' ? 'CLAUDE' : 'GPT'}</b>${hp(rem)}<em>${rem == null ? '—' : Math.round(rem) + '%'}</em>
      <span class="w-sub">${w ? `${esc(w.label)} 已用 ${Math.round(w.used_percent)}% · ${esc(until(w.resets_at))}` : p.family === 'claude' ? '下一次執行後顯示' : '尚無資料'}</span></div>`;
  }).join('') : '<span class="w-empty">沒有可用的模型</span>';
}
async function load() {
  try {
    providers = await (await fetch('/api/providers', { credentials: 'same-origin' })).json();
    providers.forEach((p) => { if (p.quota) quotas[p.id] = p.quota; });
    paint();
  } catch { /* server not up yet */ }
}
function listen() {
  const es = new EventSource('/api/events');
  es.onmessage = (m) => {
    let ev;
    try { ev = JSON.parse(m.data); } catch { return; }
    if (ev.type === 'hello') (ev.quotas || []).forEach((q) => { quotas[q.provider] = q; });
    if (ev.type === 'quota') quotas[ev.snapshot.provider] = ev.snapshot;
    paint();
  };
}
document.addEventListener('DOMContentLoaded', () => {
  load();
  listen();
  setInterval(load, 60_000);
  setInterval(paint, 30_000);
});
