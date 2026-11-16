// 待你處理 — sessions that need the user (waiting for approval first, then running), shared by
// the home attention row, the nav badge, the document title and the live region.
// Data set: every project (GET /api/sessions without ?project), so each row names its project.
import { $, api, esc, rel } from './util.js';
import { S, MODES } from './state.js';

const LIVE = new Set(['awaiting_approval', 'running']);
let timer;
let generation = 0;
const spoken = new Map(); // key → time; one announcement per key per 15 s

/**
 * Polite screen-reader announcement (never moves focus). `key` deduplicates the same fact
 * arriving from several sources (global stream, session stream) within 15 s.
 */
export function announce(text, key = text) {
  const el = $('#live');
  if (!el) return;
  const now = Date.now();
  if (now - (spoken.get(key) || 0) < 15000) return;
  spoken.set(key, now);
  el.textContent = '';
  setTimeout(() => { el.textContent = text; }, 30);
}

export function awaitingCount() {
  return (S.attention || []).filter((s) => s.status === 'awaiting_approval').length;
}

export function renderBadge() {
  const n = awaitingCount();
  const link = document.querySelector('.nav a[data-nav="home"]');
  if (link) {
    let b = link.querySelector('.badge');
    if (n && !b) { b = document.createElement('span'); b.className = 'badge'; link.appendChild(b); }
    if (b) {
      if (n) { b.textContent = n > 99 ? '99+' : String(n); b.setAttribute('aria-label', `${n} 個議題等待核准`); } else b.remove();
    }
  }
  document.title = (n ? `(${n}) ` : '') + 'Agora';
}

const projectName = (id) => S.projects.find((p) => p.id === id)?.name || '';

function itemMeta(s) {
  const waiting = s.status === 'awaiting_approval';
  return [projectName(s.project_id), MODES[s.mode]?.zh || s.mode, waiting ? '方案已整理好' : s.phase || '進行中', rel(s.updated_at)].filter(Boolean).join(' · ');
}

export function attentionHtml() {
  const list = S.attention || [];
  if (!list.length) return '';
  const [top, ...rest] = list;
  const waiting = top.status === 'awaiting_approval';
  const open = Boolean(S.attnMoreOpen) && rest.length > 0;
  return `<div class="attention ${waiting ? 'waiting' : 'running'}" role="region" aria-label="待你處理">
    <span class="eyebrow">${waiting ? 'Needs you · 等你核准' : 'In progress · 進行中'}</span>
    <a class="attention-main" href="#/s/${esc(top.id)}">
      <span class="dot ${waiting ? 'warn' : 'live'}"></span>
      <b>${esc(top.title)}</b>
      <span class="muted">${esc(itemMeta(top))}</span>
    </a>
    <a class="btn small ${waiting ? 'primary' : ''}" href="#/s/${esc(top.id)}">${waiting ? '檢視並核准 ↗' : '查看進度 ↗'}</a>
    ${rest.length ? `<button class="more-link" type="button" id="attnMoreBtn" aria-expanded="${open}" aria-controls="attnMore">還有 ${rest.length} 項 ${open ? '▴' : '▾'}</button>
    <ul class="attention-more" id="attnMore"${open ? '' : ' hidden'}>${rest.map((s) => `<li><span class="dot ${s.status === 'awaiting_approval' ? 'warn' : 'live'}"></span><a href="#/s/${esc(s.id)}">${esc(s.title)}</a><span class="muted">${esc(itemMeta(s))}</span></li>`).join('')}</ul>` : ''}
  </div>`;
}

function paintRow() {
  const row = $('#attention');
  if (!row) return;
  const hadFocus = row.contains(document.activeElement) ? document.activeElement.id : '';
  row.innerHTML = attentionHtml();
  if (hadFocus) document.getElementById(hadFocus)?.focus({ preventScroll: true });
}

// The row is rebuilt on every refresh; one delegated listener on the document handles the toggle.
document.addEventListener('click', (e) => {
  if (!e.target.closest('#attnMoreBtn')) return;
  S.attnMoreOpen = !S.attnMoreOpen;
  paintRow();
});

/**
 * Order: waiting for approval before running. Waiting: longest-waiting first (oldest updated_at).
 * Running: most recently active first. Ties break on id so the order is stable.
 */
function order(a, b) {
  if (a.status !== b.status) return a.status === 'awaiting_approval' ? -1 : 1;
  const d = a.status === 'awaiting_approval' ? a.updated_at - b.updated_at : b.updated_at - a.updated_at;
  return d || (a.id < b.id ? -1 : 1);
}

/** Reload the live sessions (all projects) and repaint every consumer. Stale responses lose. */
export async function refreshAttention() {
  const mine = ++generation;
  try {
    const all = await api('/api/sessions');
    if (mine !== generation) return;
    S.attention = all.filter((s) => LIVE.has(s.status)).sort(order);
    renderBadge();
    paintRow();
  } catch { /* offline: keep the last state */ }
}

/** Debounced refresh for bursts of status events. */
export function scheduleAttention() {
  clearTimeout(timer);
  timer = setTimeout(refreshAttention, 300);
}

// Relative times ("3 分鐘前") would freeze otherwise.
setInterval(paintRow, 60000);
