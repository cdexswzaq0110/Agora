import { api, esc } from './util.js';

const view = () => document.querySelector('#view');
const count = (n) => Number(n || 0).toLocaleString('zh-TW');

function rows(skills) {
  return skills.map((s) => `<tr class="${s.ok ? '' : 'skill-disabled'}"><th scope="row"><b>${esc(s.name)}</b><small>${esc(s.description)}</small></th><td data-label="用在哪裡">${(s.used_by || []).map((x) => `<span class="skill-tag">${esc(x)}</span>`).join('') || '—'}</td><td data-label="注入次數">${count(s.injected)}${s.truncated ? `<small>截斷 ${count(s.truncated)}</small>` : ''}</td><td data-label="審查">採用 ${count(s.review_accepted)}<br>不採用 ${count(s.review_rejected)}</td><td data-label="參考">${count(Array.isArray(s.references) ? s.references.length : s.references)}</td><td data-label="Hash"><code>${esc((s.sha256 || '').slice(0, 12))}</code></td><td data-label="狀態">${s.ok ? '<span class="status-ok">可用</span>' : `<strong class="status-bad">已停用</strong><small>${esc(s.problem || '驗證失敗')}</small>`}</td></tr>`).join('');
}

function table(skills) {
  return `<div class="skills-table-wrap"><table class="skills-table"><thead><tr><th>名稱／說明</th><th>用在哪裡</th><th>注入次數</th><th>審查</th><th>參考</th><th>Hash</th><th>狀態</th></tr></thead><tbody>${rows(skills)}</tbody></table></div>`;
}

const GUIDE_URL = 'https://github.com/cdexswzaq0110/Agora/blob/main/docs/SKILLS.md';

function guide() {
  return `<details class="block skills-guide"><summary>怎麼用這些 skill</summary><ul>
    <li><b>不用手動選，也沒有開關。</b>Agora 依團隊階段與工作坊步驟自動把對應的 skill 放進提示詞，每次最多 2 個；下表的「用在哪裡」就是對應關係。單人、協作、討論、辯論不帶 skill。</li>
    <li>每則發言上的 skill 標籤，標出那次用了哪個 skill。</li>
    <li><b>最影響結果的是驗收標準。</b>核准方案前，確認每一條都能被別人或程式判定（「<code>/api/x</code> 回 200」可以，「體驗良好」不行）；核准時可以直接修改，合約會依改後的文字封存。</li>
    <li>「已停用」代表那個 skill 的檔案與 manifest 的 hash 不符；其他 skill 照常運作，請回報。</li>
    <li>要改 skill：只改 Serendipity-Epiphany 的 <code>skills/</code> → <code>sync_skills.py build</code> → <code>install --dest harness/se</code> → 重新編譯 Agora。不要直接改 <code>harness/se/</code>。</li>
    <li>在 Claude Code、Codex 裡使用與完整步驟：<a href="${GUIDE_URL}" target="_blank" rel="noopener noreferrer">Skill 使用說明 ↗</a></li>
  </ul></details>`;
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
      ${guide()}
      <section class="block"><div class="block-head"><span class="index">Core</span><h2>核心 skills</h2></div>${table(core)}</section>
      <details class="block extended-skills"><summary>延伸 skills（${extended.length}）— 選用，不會自動注入</summary>${extended.length ? table(extended) : '<p class="muted">目前沒有延伸 skill。</p>'}</details>`;
  } catch (e) { view().innerHTML = `<section class="hero compact"><p class="eyebrow">Skills · 方法</p><h1 class="display small zh">無法載入</h1><p class="lede">${esc(e.message)}</p></section>`; }
}
