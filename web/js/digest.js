// 每日技術情報 — read today's digest, browse the archive, filter items by source, and turn
// any item into a debate ("延伸討論").
import { $, $$, api, esc, localDate, pad2, rel, toast } from './util.js';
import { md, withRefs } from './md.js';
import { S, saveComposer } from './state.js';

const view = () => $('#view');
let current = null;
let filter = 'all';

function itemMeta(i) {
  const m = [];
  if (i.score != null) m.push(i.source === 'github' ? `★ ${Number(i.score).toLocaleString('zh-TW')}` : `${i.score} 分`);
  if (i.stars_today != null) m.push(`今日 +${Number(i.stars_today).toLocaleString('zh-TW')}`);
  if (i.comments != null) m.push(`${i.comments} 留言`);
  if (i.language) m.push(i.language);
  return m.join(' · ');
}

function itemCard(i) {
  return `<article class="digest-item" id="item-${i.n}" data-source="${esc(i.source)}">
    <span class="label">${pad2(i.n)} / ${esc(i.source_label)}${i.also_on.length ? ` · 也在 ${esc(i.also_on.join('、'))}` : ''}</span>
    <h3><a href="${esc(i.url)}" target="_blank" rel="noopener noreferrer">${esc(i.title)}</a></h3>
    ${i.summary ? `<p>${esc(i.summary)}</p>` : ''}
    ${i.tags.length || i.matched_keywords.length ? `<div class="chips">${i.matched_keywords.map((k) => `<span class="chip hit">${esc(k)}</span>`).join('')}${i.tags.slice(0, 4).map((t) => `<span class="chip">${esc(t)}</span>`).join('')}</div>` : ''}
    <div class="card-foot"><span>${esc(itemMeta(i))}</span><span>${i.discussion_url ? `<a href="${esc(i.discussion_url)}" target="_blank" rel="noopener noreferrer">討論串 ↗</a>` : ''}</span>
      <button class="link" type="button" data-discuss="${i.n}">延伸討論 ↗</button></div>
  </article>`;
}

function sourcesHtml(d) {
  return d.sources.map((s) => `<span class="chip ${s.ok ? '' : 'bad'}" title="${esc(s.error || `${s.ms} ms`)}"><span class="dot ${s.ok ? 'ok' : 'bad'}"></span>${esc(s.label)} ${s.ok ? s.count : '失敗'}</span>`).join('');
}

function paint(d, index) {
  const sources = [...new Set(d.items.map((i) => i.source))];
  const labels = Object.fromEntries(d.items.map((i) => [i.source, i.source === 'reddit' ? 'Reddit' : i.source_label]));
  const items = d.items.filter((i) => filter === 'all' || i.source === filter);
  const today = localDate();
  $('#digestBody').innerHTML = `
    <section class="block">
      <div class="block-head"><span class="index">01 / Briefing</span><h2>${esc(d.date)}${d.date === today ? '（今天）' : ''}</h2><span class="aside">${d.llm_ok ? `由 ${esc(d.summarizer || '模型')} 整理` : '未經模型整理'} · ${esc(rel(d.generated_at))}</span></div>
      <div class="chips" style="margin-bottom:1rem">${sourcesHtml(d)}</div>
      ${d.llm_error ? `<div class="banner"><b>模型整理失敗，以下是未經整理的清單</b><span>${esc(d.llm_error)}</span></div>` : ''}
      <div class="md digest-md">${withRefs(md(d.markdown), d.items.length)}</div>
      ${d.usage ? `<p class="muted" style="font-size:.8rem">整理用了 輸入 ${Number(d.usage.input_tokens).toLocaleString('zh-TW')} · 輸出 ${Number(d.usage.output_tokens).toLocaleString('zh-TW')} tokens（你的訂閱額度）</p>` : ''}
    </section>
    <section class="block">
      <div class="block-head"><span class="index">02 / Sources</span><h2>今日項目</h2><span class="aside">${items.length} / ${d.items.length} 則 · 只擷取標題、熱度與簡介</span></div>
      <div class="tabs" id="srcTabs"><button class="tab${filter === 'all' ? ' active' : ''}" type="button" data-src="all">全部<sup>${d.items.length}</sup></button>${sources.map((s) => `<button class="tab${filter === s ? ' active' : ''}" type="button" data-src="${esc(s)}">${esc(labels[s])}<sup>${d.items.filter((i) => i.source === s).length}</sup></button>`).join('')}</div>
      <div class="digest-grid">${items.map(itemCard).join('')}</div>
    </section>`;
  $('#digestDate').innerHTML = index.dates.map((x) => `<option value="${esc(x)}"${x === d.date ? ' selected' : ''}>${esc(x)}</option>`).join('');
  bind(d, index);
}

function bind(d, index) {
  $('#srcTabs').onclick = (e) => { const b = e.target.closest('[data-src]'); if (b) { filter = b.dataset.src; paint(d, index); } };
  $('#digestBody').onclick = (e) => {
    const ref = e.target.closest('[data-ref]');
    if (ref) {
      e.preventDefault();
      if (filter !== 'all') { filter = 'all'; paint(d, index); }
      const el = document.getElementById(`item-${ref.dataset.ref}`);
      if (el) { el.scrollIntoView({ behavior: 'smooth', block: 'center' }); el.classList.add('flash'); setTimeout(() => el.classList.remove('flash'), 1400); }
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
    location.hash = '#/';
    toast('已帶入議題，補上「我們的情境」後開始');
  };
}

export async function renderDigest() {
  filter = 'all';
  view().innerHTML = `<section class="hero small">
      <p class="eyebrow">Daily digest · 每日技術情報</p>
      <h1 class="display zh">今天大家在關注什麼</h1>
      <p class="lede">每天從 Hacker News、Lobsters、Reddit、GitHub Trending、dev.to、Hugging Face Papers 擷取熱門項目的標題、熱度與簡介（不下載全文），再用你的 CLI 模型整理成繁體中文摘要；每個重點都附來源編號。</p>
      <div class="toolbar"><select id="digestDate" aria-label="選擇日期"></select><button class="btn small" type="button" id="runDigest">立即更新今天的情報</button><span class="muted" id="digestState"></span></div>
    </section><div id="digestBody"><p class="lede" style="padding:2rem 0">載入中…</p></div>`;
  let index;
  try { index = await api('/api/digests'); } catch (e) { $('#digestBody').innerHTML = `<p class="lede">${esc(e.message)}</p>`; return; }
  $('#digestState').textContent = index.running ? '整理中…' : index.enabled ? `每天 ${index.time} 自動整理` : '自動整理已關閉（可在設定開啟）';
  $('#runDigest').onclick = async (e) => {
    e.target.disabled = true;
    try { await api('/api/digests/run', { method: 'POST', body: {} }); $('#digestState').textContent = '整理中…（約 1–3 分鐘，完成後會自動顯示）'; }
    catch (err) { toast(err.message, true); e.target.disabled = false; }
  };
  $('#digestDate').onchange = async (e) => { try { current = await api(`/api/digests/${e.target.value}`); paint(current, index); } catch (err) { toast(err.message, true); } };
  if (!index.dates.length) {
    $('#digestBody').innerHTML = `<section class="block"><div class="empty-state"><p class="lede">還沒有任何情報。按「立即更新今天的情報」產生第一份，之後每天 ${esc(index.time)} 會自動整理。</p></div></section>`;
    return;
  }
  try { current = await api(`/api/digests/${index.dates[0]}`); paint(current, index); }
  catch (e) { $('#digestBody').innerHTML = `<p class="lede">${esc(e.message)}</p>`; }
}

/** Called by the global event stream. */
export function onDigestEvent(ev) {
  const st = $('#digestState');
  if (!st) return;
  if (ev.status === 'running') st.textContent = '整理中…';
  if (ev.status === 'failed') { st.textContent = `失敗：${ev.error || ''}`; const b = $('#runDigest'); if (b) b.disabled = false; }
  if (ev.status === 'done') renderDigest();
}

export { $$ };
