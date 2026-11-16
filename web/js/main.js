// Agora web UI entry: router, boot, global event stream, notifications, theme.
import { $, $$, api, esc, store } from './util.js';
import { S, STATUS_ZH, loadProviders, renderMeters } from './state.js';
import { renderHome, refreshHomeStatus, loadSessions } from './home.js';
import { renderSession, closeStream } from './session.js';
import { renderDigest, onDigestEvent } from './digest.js';
import { renderUsage, setQuotaInUsage } from './usage.js';
import { renderSettings, clearSettingsTimer, confirmLeaveSettings } from './settings.js';
import { renderExperiment } from './experiment.js';
import { renderPair, pairCodeFromHash } from './pair.js';
import { refreshAttention, scheduleAttention, announce, refreshDigestState } from './attention.js';
import { initShortcuts } from './shortcuts.js';

const view = () => $('#view');
window.__agora = S; // debugging aid (state of this page only)

function setNav(name) {
  $$('.nav a').forEach((a) => a.classList.toggle('active', a.dataset.nav === name));
}

async function route() {
  const hash = location.hash || '#/';
  // Undoing a cancelled navigation away from unsaved settings: nothing to render.
  if (S.restoringHash) { S.restoringHash = false; return; }
  const leavingSettings = (S.route || '').startsWith('#/settings') && !hash.startsWith('#/settings');
  if (leavingSettings && !confirmLeaveSettings()) {
    S.restoringHash = true;
    location.hash = S.route;
    return;
  }
  if (!hash.startsWith('#/settings')) clearSettingsTimer();
  closeStream();
  S.route = hash;
  if (S.whoami && !S.whoami.paired) { setNav(''); renderPair(boot); return; }
  window.scrollTo({ top: 0 });
  let m;
  if ((m = hash.match(/^#\/s\/([\w-]+)/))) { setNav('home'); await renderSession(m[1]); }
  else if ((m = hash.match(/^#\/x\/([\w-]+)/))) { setNav('home'); await renderExperiment(m[1]); }
  else if (hash.startsWith('#/digest')) { setNav('digest'); await renderDigest(); }
  else if (hash.startsWith('#/usage')) { setNav('usage'); await renderUsage(); }
  else if (hash.startsWith('#/settings')) { setNav('settings'); await renderSettings(); }
  else { setNav('home'); await renderHome(); }
  focusHeading();
}

/** After a route change only (never on live updates): move focus to the page heading. */
function focusHeading() {
  const h = document.querySelector('#view h1');
  if (!h) return;
  if (!h.hasAttribute('tabindex')) h.setAttribute('tabindex', '-1');
  h.focus({ preventScroll: true });
}

function openGlobal() {
  const es = new EventSource('/api/events');
  es.onmessage = (m) => {
    let ev;
    try { ev = JSON.parse(m.data); } catch { return; }
    // hello arrives on every (re)connect; lagged means events were dropped — resync either way.
    if (ev.type === 'hello') { (ev.quotas || []).forEach((q) => { S.quotas[q.provider] = q; }); renderMeters(); scheduleAttention(); refreshDigestState(); }
    if (ev.type === 'lagged') { scheduleAttention(); refreshDigestState(); if ((location.hash || '#/') === '#/' && $('#sessions')) loadSessions(); }
    if (ev.type === 'quota') setQuotaInUsage(ev.snapshot);
    if (ev.type === 'digest') {
      onDigestEvent(ev);
      if (ev.status === 'done') notify('Agora · 每日技術情報', `${ev.date} 的技術情報已整理好`, '#/digest', `digest-${ev.date}`);
    }
    if (ev.type === 'status') {
      scheduleAttention();
      if (ev.status === 'awaiting_approval') announce(`方案已整理好，等你核准：${ev.title}`, `${ev.session_id}:${ev.status}`);
      if ((location.hash || '#/') === '#/' && $('#sessions')) loadSessions();
      if (ev.status === 'awaiting_approval') notify('Agora · 等你核准', `方案已整理好：${ev.title}`, `#/s/${ev.session_id}`, ev.session_id);
      else if (ev.status !== 'running') notify(`Agora · ${STATUS_ZH[ev.status] || ev.status}`, ev.title, `#/s/${ev.session_id}`, ev.session_id);
    }
  };
}

function notify(title, body, hash, tag) {
  if (S.desktop) return; // the desktop shell raises native notifications
  if (!('Notification' in window) || Notification.permission !== 'granted' || !document.hidden) return;
  try {
    const n = new Notification(title, { body, icon: '/icon-192.png', tag });
    n.onclick = () => { window.focus(); location.hash = hash; };
  } catch { /* some mobile browsers only allow SW notifications */ }
}

const THEMES = ['auto', 'light', 'dark'];
const THEME_ZH = { auto: '自動', light: '淺色', dark: '深色' };
function applyTheme(t) {
  if (t === 'auto') document.documentElement.removeAttribute('data-theme');
  else document.documentElement.setAttribute('data-theme', t);
  const b = $('#themeBtn');
  if (b) b.textContent = `主題：${THEME_ZH[t]}`;
}

let globalOpen = false;
async function boot() {
  try {
    S.whoami = await api('/api/whoami');
  } catch (e) {
    view().innerHTML = `<section class="hero"><p class="eyebrow">AGORA</p><h1 class="display zh">連不上 Agora</h1><p class="lede">${esc(e.message)}</p></section>`;
    return;
  }
  $('#footVersion').textContent = `Agora v${S.whoami.version}${S.whoami.local ? '' : ' · ' + (S.whoami.device || '已配對裝置')}`;
  if (!S.whoami.paired) { setNav(''); renderPair(boot); return; }
  await Promise.all([
    loadProviders(),
    api('/api/projects').then((p) => { S.projects = p; }),
    api('/api/templates').then((t) => { S.templates = t; }),
  ]);
  if (!globalOpen) { openGlobal(); globalOpen = true; }
  refreshAttention();
  refreshDigestState();
  // Health checks spawn CLIs and take a few seconds — refresh after first paint.
  if (S.providers.some((p) => !p.health)) {
    setTimeout(async () => {
      try { await loadProviders(true); refreshHomeStatus(); if (S.route.startsWith('#/usage')) renderUsage(); } catch { /* ignore */ }
    }, 50);
  }
  if (pairCodeFromHash()) history.replaceState(null, '', location.pathname + location.search + '#/');
  await route();
}

window.addEventListener('hashchange', route);
document.addEventListener('visibilitychange', () => { if (!document.hidden && S.whoami?.paired) { scheduleAttention(); refreshDigestState(); } });
// Sticky elements below the top bar need its real height (it wraps to two rows on phones).
function measureTopbar() {
  const t = $('.topbar');
  if (t) document.documentElement.style.setProperty('--topbar-h', `${t.offsetHeight}px`);
}
// Its height changes after load too (quota meters render late, the nav badge appears).
if ('ResizeObserver' in window) new ResizeObserver(measureTopbar).observe(document.querySelector('.topbar') || document.documentElement);
else window.addEventListener('resize', measureTopbar);

// Phones: hide the bottom nav while the on-screen keyboard is up. Both must hold — a text field
// has focus AND the visual viewport lost a quarter of the window — so a collapsing URL bar alone
// never counts as a keyboard.
function watchKeyboard() {
  const vv = window.visualViewport;
  if (!vv) return;
  const typing = () => {
    const a = document.activeElement;
    return Boolean(a && (a.isContentEditable || a.tagName === 'TEXTAREA' || a.tagName === 'SELECT'
      || (a.tagName === 'INPUT' && !['checkbox', 'radio', 'button', 'submit', 'range'].includes(a.type))));
  };
  const update = () => document.body.classList.toggle('kb-open', typing() && vv.height < window.innerHeight * 0.75);
  vv.addEventListener('resize', update);
  document.addEventListener('focusin', update);
  document.addEventListener('focusout', () => setTimeout(update, 60));
}

document.addEventListener('DOMContentLoaded', () => {
  measureTopbar();
  watchKeyboard();
  initShortcuts();
  let theme = store.get('theme', 'auto');
  applyTheme(theme);
  $('#themeBtn').addEventListener('click', () => { theme = THEMES[(THEMES.indexOf(theme) + 1) % THEMES.length]; store.set('theme', theme); applyTheme(theme); });
  if ('serviceWorker' in navigator && window.isSecureContext && !S.desktop) navigator.serviceWorker.register('/sw.js').catch(() => {});
  boot();
});
