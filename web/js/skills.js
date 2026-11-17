import { api, esc } from './util.js';

const view = () => document.querySelector('#view');
const count = (n) => Number(n || 0).toLocaleString('zh-TW');

function rows(skills) {
  return skills.map((s) => `<tr class="${s.ok ? '' : 'skill-disabled'}"><th scope="row"><b>${esc(s.name)}</b><small>${esc(s.description)}</small></th><td data-label="用在哪裡">${(s.used_by || []).map((x) => `<span class="skill-tag">${esc(x)}</span>`).join('') || '—'}</td><td data-label="注入次數">${count(s.injected)}${s.truncated ? `<small>截斷 ${count(s.truncated)}</small>` : ''}</td><td data-label="審查">採用 ${count(s.review_accepted)}<br>不採用 ${count(s.review_rejected)}</td><td data-label="參考">${count(Array.isArray(s.references) ? s.references.length : s.references)}</td><td data-label="Hash"><code>${esc((s.sha256 || '').slice(0, 12))}</code></td><td data-label="狀態">${s.ok ? '<span class="status-ok">可用</span>' : `<strong class="status-bad">已停用</strong><small>${esc(s.problem || '驗證失敗')}</small>`}</td></tr>`).join('');
}

function table(skills) {
  return `<div class="skills-table-wrap"><table class="skills-table"><thead><tr><th>名稱／說明</th><th>用在哪裡</th><th>注入次數</th><th>審查</th><th>參考</th><th>Hash</th><th>狀態</th></tr></thead><tbody>${rows(skills)}</tbody></table></div>`;
}

export async function renderSkills() {
  view().innerHTML = '<section class="hero compact"><p class="eyebrow">Skills · 方法</p><h1 class="display small zh">載入中…</h1></section>';
  try {
    const data = await api('/api/skills');
    const repo = /^https?:\/\//.test(data.source_repo || '') ? data.source_repo : '';
    const commit = (data.source_commit || '').slice(0, 8) || '—';
    const source = repo ? `<a href="${esc(repo)}" target="_blank" rel="noopener noreferrer">${esc(commit)} ↗</a>` : `<code>${esc(commit)}</code>`;
    const core = data.skills.filter((s) => s.tier === 'core');
    const extended = data.skills.filter((s) => s.tier === 'extended');
    view().innerHTML = `<section class="hero compact"><p class="eyebrow">Skills · 方法</p><h1 class="display small zh">Skills</h1><p class="lede">Agora 注入模型的方法與審查使用紀錄。</p><div class="skills-meta"><span>Bundle <code>${esc(data.bundle_version || '—')}</code></span><span>來源 ${source}</span><span class="${data.ok ? 'status-ok' : 'status-bad'}">${data.ok ? '驗證通過' : `驗證失敗：${esc(data.problem || '未知問題')}`}</span></div></section>
      <section class="block"><div class="block-head"><span class="index">Core</span><h2>核心 skills</h2></div>${table(core)}</section>
      <details class="block extended-skills"><summary>延伸 skills（${extended.length}）— 選用，不會自動注入</summary>${extended.length ? table(extended) : '<p class="muted">目前沒有延伸 skill。</p>'}</details>`;
  } catch (e) { view().innerHTML = `<section class="hero compact"><p class="eyebrow">Skills · 方法</p><h1 class="display small zh">無法載入</h1><p class="lede">${esc(e.message)}</p></section>`; }
}
