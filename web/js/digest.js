// 每日技術情報 (v0.4): compact header with the day-over-day comparison, a section index,
// the full item list folded behind filters (source / 只看新上榜 / 只看關注主題), trend badges,
// and a shared read cursor (marked only after this exact version has been painted).
// See docs/design/2026-11-16-v0.4/05-final-plan.md (B, C, D).
import { $, $$, api, esc, localDate, pad2, rel, toast } from './util.js';
import { md, withRefs } from './md.js';
import { S, saveComposer } from './state.js';
import { refreshDigestState } from './attention.js';

const view = () => $('#view');
let current = null;
let index = null;
const ui = { source: 'all', onlyNew: false, onlyTopics: false, listOpen: false };
const motion = () => (matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth');

function itemMeta(i) {
  const m = [];
  if (i.score != null) m.push(i.source === 'github' ? `★ ${Number(i.score).toLocaleString('zh-TW')}` : `${i.score} 分`);
  if (i.stars_today != null) m.push(`今日 +${Number(i.stars_today).toLocaleString('zh-TW')}`);
  if (i.comments != null) m.push(`${i.comments} 留言`);
  if (i.language) m.push(i.language);
  return m.join(' · ');
}

const UNIT = { stars_today: ' stars', comments: ' 則留言', score: ' 分' };

/** Text + symbol, never colour alone. */
function trendBadges(i) {
  const t = i.trend;
  if (!t) return '';
  const out = [];
  if (t.kind === 'new') out.push('<span class="trend new">✦ 新上榜</span>');
  if (t.kind === 'rising') out.push(`<span class="trend rising">↑ 升溫 +${esc(t.delta)}${esc(UNIT[t.metric] || '')}</span>`);
  if (t.streak >= 2) out.push(`<span class="trend streak">連續 ${esc(t.streak)} 天</span>`);
  return out.length ? `<div class="trends">${out.join('')}</div>` : '';
}

function itemCard(i) {
  return `<article class="digest-item" id="item-${i.n}" data-source="${esc(i.source)}">
    <span class="label">${pad2(i.n)} / ${esc(i.source_label)}${i.also_on.length ? ` · 也在 ${esc(i.also_on.join('、'))}` : ''}</span>
    ${trendBadges(i)}
    <h3><a href="${esc(i.url)}" target="_blank" rel="noopener noreferrer">${esc(i.title)}</a></h3>
    ${i.summary ? `<p>${esc(i.summary)}</p>` : ''}
    ${i.tags.length || i.matched_keywords.length ? `<div class="chips">${i.matched_keywords.map((k) => `<span class="chip hit">${esc(k)}</span>`).join('')}${i.tags.slice(0, 4).map((t) => `<span class="chip">${esc(t)}</span>`).join('')}</div>` : ''}
    <div class="card-foot"><span>${esc(itemMeta(i))}</span><span>${i.discussion_url ? `<a href="${esc(i.discussion_url)}" target="_blank" rel="noopener noreferrer">討論串 ↗</a>` : ''}</span>
      <button class="link" type="button" data-discuss="${i.n}">延伸討論 ↗</button></div>
  </article>`;
}

function sourcesHtml(d) {
  return d.sources.map((s) => `<span class="chip ${s.ok ? '' : 'bad'}" title="${esc(s.error || `${s.ms} ms`)}"><span class="dot ${s.ok ? 'ok' : 'bad'}" aria-hidden="true"></span>${esc(s.label)} ${s.ok ? s.count : '失敗'}</span>`).join('');
}

function compareHtml(d) {
  if (!d.compared_with) return '<span class="muted">沒有前一份情報可以比較</span>';
  const n = (k) => d.items.filter((i) => i.trend?.kind === k).length;
  return `<span class="compare-line">和 ${esc(d.compared_with)} 相比：<b>新上榜 ${n('new')}</b> · <b>升溫 ${n('rising')}</b></span>`;
}

function filtered(d) {
  return d.items.filter((i) => (ui.source === 'all' || i.source === ui.source)
    && (!ui.onlyNew || i.trend?.kind === 'new')
    && (!ui.onlyTopics || i.matched_keywords.length));
}

function listHtml(d) {
  const sources = [...new Set(d.items.map((i) => i.source))];
  const labels = Object.fromEntries(d.items.map((i) => [i.source, i.source === 'reddit' ? 'Reddit' : i.source_label]));
  const items = filtered(d);
  return `<div class="list-filters">
      <div class="tabs" id="srcTabs" role="group" aria-label="依來源篩選"><button class="tab${ui.source === 'all' ? ' active' : ''}" type="button" data-src="all" aria-pressed="${ui.source === 'all'}">全部<sup>${d.items.length}</sup></button>${sources.map((s) => `<button class="tab${ui.source === s ? ' active' : ''}" type="button" data-src="${esc(s)}" aria-pressed="${ui.source === s}">${esc(labels[s])}<sup>${d.items.filter((i) => i.source === s).length}</sup></button>`).join('')}</div>
      <label class="check"><input type="checkbox" id="onlyNew"${ui.onlyNew ? ' checked' : ''}${d.compared_with ? '' : ' disabled'}> 只看新上榜</label>
      <label class="check"><input type="checkbox" id="onlyTopics"${ui.onlyTopics ? ' checked' : ''}> 只看我關注的主題</label>
      <span class="muted" aria-live="polite">${items.length} / ${d.items.length} 則</span>
    </div>
    <div class="digest-grid">${items.map(itemCard).join('') || '<p class="muted">沒有符合篩選的項目。</p>'}</div>`;
}

function paint(d) {
  const today = localDate();
  $('#digestDate').innerHTML = index.dates.map((x) => `<option value="${esc(x)}"${x === d.date ? ' selected' : ''}>${esc(x)}${x === today ? '（今天）' : ''}</option>`).join('');
  $('#digestMeta').innerHTML = `${compareHtml(d)}<span class="muted">${d.llm_ok ? `由 ${esc(d.summarizer || '模型')} 整理` : '未經模型整理'} · ${esc(rel(d.generated_at))}</span>`;
  $('#digestBody').innerHTML = `
    <div class="digest-layout">
      <nav class="digest-toc" aria-label="情報章節"><h3>章節</h3><ol id="tocList"></ol></nav>
      <div class="digest-main">
        <label class="toc-select"><span class="sr">跳到章節</span><select id="tocSelect"><option value="">跳到章節…</option></select></label>
        <div class="chips sources-row">${sourcesHtml(d)}</div>
        ${d.llm_error ? `<div class="banner"><b>模型整理失敗，以下是未經整理的清單</b><span>${esc(d.llm_error)}</span></div>` : ''}
        <div class="md digest-md" id="digestMd">${withRefs(md(d.markdown), d.items.length)}</div>
        ${d.usage ? `<p class="muted" style="font-size:.8rem">整理用了 輸入 ${Number(d.usage.input_tokens).toLocaleString('zh-TW')} · 輸出 ${Number(d.usage.output_tokens).toLocaleString('zh-TW')} tokens（你的訂閱額度）</p>` : ''}
        <section class="all-items" id="sec-items" aria-labelledby="itemsTitle">
          <h2 id="itemsTitle" class="items-head"><button type="button" class="items-toggle" id="itemsToggle" aria-expanded="${ui.listOpen}" aria-controls="itemsList">全部 ${d.items.length} 項 <span class="muted">只擷取標題、熱度與簡介</span></button></h2>
          <div id="itemsList"${ui.listOpen ? '' : ' hidden'}>${listHtml(d)}</div>
        </section>
      </div>
    </div>`;
  // Section index from the rendered headings (ids assigned here; text stays escaped by md()).
  const heads = $$('#digestMd h2');
  heads.forEach((h, k) => { h.id = `sec-${k}`; });
  const entries = [...heads.map((h) => [h.id, h.textContent]), ['sec-items', `全部 ${d.items.length} 項`]];
  $('#tocList').innerHTML = entries.map(([id, t]) => `<li><a href="#${esc(id)}" data-toc="${esc(id)}">${esc(t)}</a></li>`).join('');
  $('#tocSelect').innerHTML += entries.map(([id, t]) => `<option value="${esc(id)}">${esc(t)}</option>`).join('');
  bind(d);
  markRead(d);
}

/** Only after this exact version is on screen: the server checks `generated_at` matches. */
async function markRead(d) {
  try {
    await api(`/api/digests/${encodeURIComponent(d.date)}/read`, { method: 'POST', body: { generated_at: d.generated_at } });
  } catch { /* regenerated meanwhile: the page will reload on the digest event */ }
  refreshDigestState();
}

function repaintList(d) {
  const box = $('#itemsList');
  if (!box) return;
  const focusId = document.activeElement?.id;
  box.innerHTML = listHtml(d);
  bindList(d);
  if (focusId) document.getElementById(focusId)?.focus();
}

function openList(d) {
  if (ui.listOpen) return;
  ui.listOpen = true;
  $('#itemsToggle').setAttribute('aria-expanded', 'true');
  $('#itemsList').hidden = false;
}

function jumpTo(id, d) {
  if (id === 'sec-items') openList(d);
  const el = document.getElementById(id);
  if (!el) return;
  el.scrollIntoView({ behavior: motion(), block: 'start' });
  if (!el.hasAttribute('tabindex')) el.setAttribute('tabindex', '-1');
  el.focus({ preventScroll: true });
}

function bindList(d) {
  const tabs = $('#srcTabs');
  if (tabs) tabs.onclick = (e) => { const b = e.target.closest('[data-src]'); if (b) { ui.source = b.dataset.src; repaintList(d); } };
  const on = $('#onlyNew');
  if (on) on.onchange = () => { ui.onlyNew = on.checked; repaintList(d); };
  const ot = $('#onlyTopics');
  if (ot) ot.onchange = () => { ui.onlyTopics = ot.checked; repaintList(d); };
}

function bind(d) {
  bindList(d);
  $('#itemsToggle').onclick = (e) => {
    ui.listOpen = !ui.listOpen;
    e.currentTarget.setAttribute('aria-expanded', String(ui.listOpen));
    $('#itemsList').hidden = !ui.listOpen;
  };
  $('#tocList').onclick = (e) => {
    const a = e.target.closest('[data-toc]');
    if (!a) return;
    e.preventDefault(); // hash links would hit the router
    jumpTo(a.dataset.toc, d);
  };
  $('#tocSelect').onchange = (e) => { if (e.target.value) jumpTo(e.target.value, d); e.target.value = ''; };
  $('#digestBody').onclick = (e) => {
    const ref = e.target.closest('[data-ref]');
    if (ref) {
      e.preventDefault();
      openList(d);
      if (ui.source !== 'all' || ui.onlyNew || ui.onlyTopics) { Object.assign(ui, { source: 'all', onlyNew: false, onlyTopics: false }); repaintList(d); }
      const el = document.getElementById(`item-${ref.dataset.ref}`);
      if (el) { el.scrollIntoView({ behavior: motion(), block: 'center' }); el.classList.add('flash'); setTimeout(() => el.classList.remove('flash'), 1400); }
      return;
    }
    const b = e.target.closest('[data-discuss]');
    if (!b) return;
    const i = d.items.find((x) => x.n === Number(b.dataset.discuss));
    const t = S.templates.find((x) => x.id === 'digest-discuss');
    S.c.mode = 'debate';
    S.c.rounds.debate = t?.rounds ?? 1;
    S.c.template = 'digest-discuss';
    S.c.seed = null;
    S.c.prompt = `這項技術／專案值得我們採用嗎？\n\n「${i.title}」\n來源：${i.url}${i.discussion_url ? `\n討論串：${i.discussion_url}` : ''}${i.summary ? `\n簡介：${i.summary}` : ''}\n\n我們的情境：`;
    saveComposer();
    location.hash = '#/sessions';
    toast('已帶入專案，補上「我們的情境」後開始');
  };
}

async function load(date) {
  try { current = await api(`/api/digests/${encodeURIComponent(date)}`); paint(current); }
  catch (e) { $('#digestBody').innerHTML = `<p class="lede">${esc(e.message)}</p>`; }
}

export async function renderDigest() {
  Object.assign(ui, { source: 'all', onlyNew: false, onlyTopics: false, listOpen: false });
  view().innerHTML = `<section class="hero compact digest-hero">
      <p class="eyebrow">Daily digest · 每日技術情報</p>
      <h1 class="display small zh" tabindex="-1">今天大家在關注什麼</h1>
      <div class="toolbar"><label><span class="sr">選擇日期</span><select id="digestDate"></select></label><button class="btn small" type="button" id="runDigest">立即更新今天的情報</button><span class="muted" id="digestState"></span></div>
      <div class="digest-meta" id="digestMeta"></div>
    </section><div id="digestBody"><p class="lede" style="padding:2rem 0">載入中…</p></div>`;
  try { index = await api('/api/digests'); } catch (e) { $('#digestBody').innerHTML = `<p class="lede">${esc(e.message)}</p>`; return; }
  $('#digestState').textContent = index.running ? '整理中…' : index.enabled ? `每天 ${index.time} 自動整理` : '自動整理已關閉（可在設定開啟）';
  $('#runDigest').onclick = async (e) => {
    e.target.disabled = true;
    try { await api('/api/digests/run', { method: 'POST', body: {} }); $('#digestState').textContent = '整理中…（約 1–3 分鐘，完成後會自動顯示）'; }
    catch (err) { toast(err.message, true); e.target.disabled = false; }
  };
  $('#digestDate').onchange = (e) => load(e.target.value);
  if (!index.dates.length) {
    $('#digestBody').innerHTML = `<section class="block"><div class="empty-state"><p class="lede">還沒有任何情報。按「立即更新今天的情報」產生第一份，之後每天 ${esc(index.time)} 會自動整理。</p></div></section>`;
    return;
  }
  await load(index.dates[0]);
}

/** Called by the global event stream. */
export function onDigestEvent(ev) {
  if (ev.status === 'done') refreshDigestState();
  const st = $('#digestState');
  if (!st) return;
  if (ev.status === 'running') st.textContent = '整理中…';
  if (ev.status === 'failed') { st.textContent = `失敗：${ev.error || ''}`; const b = $('#runDigest'); if (b) b.disabled = false; }
  if (ev.status === 'done') renderDigest();
}

export { $$ };
