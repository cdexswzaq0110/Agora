// 同題比較 — the same question through several modes, side by side.
import { $, api, compact, esc } from './util.js';
import { GRADE } from './md.js';
import { MODES, STATUS_ZH, chip } from './state.js';

const view = () => $('#view');
let timer;

export async function renderExperiment(id) {
  clearTimeout(timer);
  view().innerHTML = '<section class="hero small"><p class="eyebrow">Comparison · 同題比較</p><h1 class="display zh">載入中…</h1></section><div id="xBody"></div>';
  const load = async () => {
    if (!location.hash.startsWith(`#/x/${id}`)) return;
    let v;
    try { v = await api(`/api/experiments/${encodeURIComponent(id)}`); } catch (e) { $('#xBody').innerHTML = `<p class="lede">${esc(e.message)}</p>`; return; }
    paint(v);
    if (!v.experiment.done) timer = setTimeout(load, 2500);
  };
  load();
}

function paint(v) {
  const x = v.experiment;
  const byMode = Object.fromEntries(v.sessions.map((s) => [s.mode, s]));
  $('.hero h1').textContent = x.title;
  $('.hero').insertAdjacentHTML('beforeend', '');
  $('#xBody').innerHTML = `
    <section class="block">
      <div class="block-head"><span class="index">01 / Result</span><h2>${x.done ? '比較結果' : '依序執行中…'}</h2><span class="aside">同樣的參與者，唯讀；工作坊只產出方案</span></div>
      <div class="chips" style="margin-bottom:1rem">${(v.sessions[0]?.participants || []).map(chip).join('')}</div>
      <table class="data compare-table"><thead><tr><th>模式</th><th>狀態</th><th>證據等級</th><th>發言</th><th>Tokens</th><th></th></tr></thead><tbody>
      ${x.modes.map((m) => {
        const s = byMode[m];
        if (!s) return `<tr><td>${esc(MODES[m].zh)}</td><td colspan="5" class="muted">等待中</td></tr>`;
        const g = s.grade ? GRADE[s.grade] : null;
        return `<tr><td>${esc(MODES[m].zh)}</td><td>${s.status === 'running' ? '<span class="dot live"></span> ' : ''}${esc(STATUS_ZH[s.status])}</td><td>${g ? `<span class="eg ${g.cls}">${g.zh}</span>` : '—'}</td><td>${s.turns}</td><td>${compact(s.input_tokens + s.output_tokens)}</td><td><a href="#/s/${esc(s.id)}">開啟 ↗</a></td></tr>`;
      }).join('')}</tbody></table>
      ${x.errors.length ? `<div class="banner bad"><b>有模式沒有跑起來</b>${x.errors.map((e) => `<span>${esc(e)}</span>`).join('')}</div>` : ''}
      <p class="muted" style="font-size:.85rem">讀法：等級越高代表結論越有依據；tokens 代表成本。「推論」以上且成本較低的模式，就是這類問題的好選擇。這個比較只有一題，結論是【候選】；同類問題多比幾次才可靠。</p>
    </section>
    <section class="block"><div class="block-head"><span class="index">02 / Question</span><h2>題目</h2></div><pre class="prompt-pre">${esc(x.prompt)}</pre></section>`;
}
