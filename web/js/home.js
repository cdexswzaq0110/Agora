// Home (v0.3 workbench): compact intro → 待你處理 → composer (project, template, mode, brief,
// flow summary, start; details behind 調整) → archive. See docs/design/2026-11-16-ui-v0.3.
import { $, $$, api, esc, pad2, rel, store, toast } from './util.js';
import { GRADE } from './md.js';
import {
  S, MODES, MODE_ORDER, STATUS_ZH, FAMILY, saveComposer, providerById, famOf, pickProvider, ensureCast,
  providerOptions, modelOptions, chip,
} from './state.js';
import { voiceButton, bindVoice } from './voice.js';
import { attentionHtml, refreshAttention } from './attention.js';

const view = () => $('#view');
const SHORT = { workshop: '方案工作坊', solo: '單人', relay: '協做', roundtable: '討論', debate: '辯論' };

// ------------------------------------------------------------------ cast & options (inside 調整)

function slotRoles(mode, i) {
  if (mode === 'solo') return '模型';
  if (mode === 'relay') return i === 0 ? '執行者' : '審查者';
  if (mode === 'debate') return i === 0 ? '正方' : '反方';
  if (mode === 'workshop') return `提案者 ${i + 1}`;
  return `與談人 ${i + 1}`;
}

function roleSelect(id, label, value, empty, hint) {
  return `<div class="slot"><div class="slot-head"><span class="slot-role">${esc(label)}</span></div>
    <label class="field"><span>由誰擔任</span><select id="${id}">${providerOptions(value, { allowEmpty: empty })}</select></label>
    ${hint ? `<p class="hint">${esc(hint)}</p>` : ''}</div>`;
}

function castHtml() {
  const mode = S.c.mode;
  const slots = S.c.cast[mode];
  const multi = mode === 'roundtable' || mode === 'workshop';
  let html = slots.map((s, i) => {
    const removable = multi && slots.length > (mode === 'workshop' ? 1 : 2);
    return `<div class="slot fam-${famOf(s.provider)}" data-slot="${i}">
      <div class="slot-head"><span class="slot-role">${esc(slotRoles(mode, i))}</span>${removable ? `<button class="slot-x" type="button" data-remove="${i}" aria-label="移除${esc(slotRoles(mode, i))}">✕</button>` : ''}</div>
      <label class="field"><span>模型提供者</span><select data-prov="${i}">${providerOptions(s.provider)}</select></label>
      <label class="field"><span>模型</span><select data-model="${i}">${modelOptions(s.provider, s.model)}</select></label>
    </div>`;
  }).join('');
  if (multi && slots.length < 4) html += `<button class="add-slot" type="button" id="addSlot">＋ 新增${mode === 'workshop' ? '提案者' : '與談人'}</button>`;
  if (mode === 'debate') html += roleSelect('judgeSel', '裁判', S.c.judge.debate, '交叉裁判（雙方各自裁決）', '交叉裁判可以抵銷「偏袒自家模型」的傾向。');
  if (mode === 'roundtable') html += roleSelect('judgeSel', '總結者', S.c.judge.roundtable, '由與談人 1 總結', '整理共識、分歧、建議與待驗證事項。');
  if (mode === 'workshop') {
    html += roleSelect('judgeSel', '總結者', S.c.judge.workshop, '由提案者 1 整合', '把提案與評論整合成附驗收標準的最終方案。');
    if (!S.c.planOnly) {
      html += roleSelect('builderSel', '執行者', S.c.builder, '由提案者 1 實作', '核准後在隔離的 git 分支實作。');
      html += roleSelect('reviewerSel', '審查者', S.c.reviewer, '由另一家模型審查', '依驗收標準逐條檢查實作。');
    }
  }
  return html;
}

export function budgetEstimate() {
  const mode = S.c.mode;
  const r = Number(S.c.rounds[mode] ?? 1);
  const n = S.c.cast[mode].length;
  let b;
  if (mode === 'solo') b = 1;
  else if (mode === 'relay') b = 2 * r + 2;
  else if (mode === 'roundtable') b = n * r + 1;
  else if (mode === 'debate') b = 2 * r + 2;
  else b = 2 * n + 1 + (S.c.redTeam ? 2 : 0) + 2 + (S.c.planOnly ? 0 : 2 * r + 2);
  const user = Number(S.c.maxTurns);
  if (user > 0) b = Math.min(b, user);
  return Math.min(b, 24);
}

const check = (id, on, label, hint = '') => `<label class="check" title="${esc(hint)}"><input type="checkbox" id="${id}"${on ? ' checked' : ''}> ${esc(label)}</label>`;

function optionsHtml() {
  const mode = S.c.mode;
  const parts = [];
  const writes = mode === 'workshop' ? !S.c.planOnly : (mode === 'solo' || mode === 'relay') && S.c.access === 'write';
  if (mode === 'relay' || mode === 'workshop') {
    parts.push(`<label class="field"><span>最多修訂次數</span><select id="optRounds">${[0, 1, 2, 3].map((n) => `<option value="${n}"${Number(S.c.rounds[mode]) === n ? ' selected' : ''}>${n} 次</option>`).join('')}</select></label>`);
  }
  if (mode === 'roundtable' || mode === 'debate') {
    parts.push(`<label class="field"><span>${mode === 'debate' ? '攻防輪數' : '討論輪數'}</span><select id="optRounds">${[1, 2, 3, 4].map((n) => `<option value="${n}"${Number(S.c.rounds[mode]) === n ? ' selected' : ''}>${n} 輪</option>`).join('')}</select></label>`);
  }
  if (mode === 'solo' || mode === 'relay') {
    parts.push(`<div class="field"><span class="field-label">檔案權限</span>
      <div class="seg" role="group" aria-label="檔案權限"><button type="button" data-access="read_only" aria-pressed="${S.c.access === 'read_only'}" class="${S.c.access === 'read_only' ? 'on' : ''}">唯讀</button><button type="button" data-access="write" aria-pressed="${S.c.access === 'write'}" class="${S.c.access === 'write' ? 'on' : ''}">可修改檔案</button></div></div>`);
  }
  if (mode === 'solo' || mode === 'relay' || (mode === 'workshop' && !S.c.planOnly)) {
    parts.push(`<label class="field"><span>驗證指令（選填）</span><input type="text" id="optVerify" placeholder="${esc(S.c.verifyHint || '例如 cargo test、npm test、python -m pytest')}" value="${esc(S.c.verify)}"></label>`);
  }
  parts.push(`<label class="field"><span>發言上限（選填）</span><input type="number" id="optMax" min="1" max="24" placeholder="自動" value="${esc(S.c.maxTurns)}"></label>`);
  const flags = [];
  if (mode === 'workshop') {
    flags.push(check('optRed', S.c.redTeam, '紅隊審視', '整合後再由另一個模型專門找漏洞，總結者修訂一次'));
    flags.push(check('optPlanOnly', S.c.planOnly, '只產出方案（不實作）'));
    flags.push(check('optAuto', S.c.autoApprove, '自動核准（不等我）', '不建議：核准關卡是你檢查方案的機會'));
  }
  if (writes) flags.push(check('optIsolate', S.c.isolate, '在隔離的 git worktree 實作', 'git 專案預設開啟：完成後檢查 diff 再決定是否套用'));
  if (mode !== 'solo') flags.push(check('optBlind', S.c.blind, '盲審（隱藏作者是哪個模型）'));
  if (flags.length) parts.push(`<div class="field flags">${flags.join('')}</div>`);
  return parts.join('');
}

function compareHtml() {
  const on = S.c.compare || [];
  return `<details class="brief compare" ${on.length ? 'open' : ''}><summary>同一題比較多種模式（唯讀、依序執行）</summary>
    <div class="flags" style="margin-top:.6rem">${MODE_ORDER.map((m) => check(`cmp-${m}`, on.includes(m), MODES[m].zh)).join('')}</div>
    <p class="hint">勾選 2 種以上時，「開始」會建立一個比較實驗：同樣的參與者依序跑每種模式（工作坊只產出方案），最後並排比較等級、發言數與 tokens。</p></details>`;
}

// ------------------------------------------------------------------ flow summary

const who = (id) => {
  const p = providerById(id);
  if (!p) return id || '—';
  return (p.mock ? '模擬 ' : '') + (FAMILY[p.family] || p.family);
};

function counterpart(builderId) {
  const cast = S.c.cast.workshop;
  return (cast.find((s) => s.provider !== builderId) || cast[1] || cast[0] || {}).provider;
}

/** One line that says exactly what will run — the composer's contract. */
export function flowSummary() {
  const m = S.c.mode;
  const cast = S.c.cast[m];
  const r = Number(S.c.rounds[m] ?? 1);
  const names = cast.map((s) => who(s.provider));
  let flow;
  const compare = S.c.compare || [];
  if (compare.length >= 2) {
    // Mirrors Engine::start_experiment: every mode read-only; workshop → plan-only + auto-approve.
    const zh = compare.map((x) => MODES[x]?.zh || x);
    return {
      flow: `同題比較：${zh.join('、')} 依序執行，參與者 ${names.join('、')}`,
      tags: ['全部唯讀', ...(compare.includes('workshop') ? ['工作坊只產出方案、自動核准'] : [])],
    };
  }
  if (m === 'workshop') {
    const synth = who(S.c.judge.workshop || cast[0]?.provider);
    const builder = S.c.builder || cast[0]?.provider;
    flow = `提案 ${names.join('、')} → 總結 ${synth}${S.c.redTeam ? ' → 紅隊' : ''} → ${S.c.autoApprove ? '自動核准（不等你）' : '你核准'}` + (S.c.planOnly ? '（只產出方案）' : ` → 實作 ${who(builder)} → 審查 ${who(S.c.reviewer || counterpart(builder))}`);
  } else if (m === 'solo') {
    flow = `${names[0]} 直接${S.c.access === 'write' ? '動手（可改檔）' : '回答（唯讀）'}`;
  } else if (m === 'relay') {
    flow = `${names[0]} 執行${S.c.access === 'write' ? '（可改檔）' : ''} → ${names[1] || names[0]} 審查，最多修訂 ${r} 次`;
  } else if (m === 'roundtable') {
    flow = `${names.join('、')} 討論 ${r} 輪 → ${who(S.c.judge.roundtable || cast[0]?.provider)} 總結`;
  } else {
    flow = `正方 ${names[0]} vs 反方 ${names[1] || names[0]}，${r} 輪 → ${S.c.judge.debate ? `${who(S.c.judge.debate)} 裁決` : '交叉裁判'}`;
  }
  const writes = m === 'workshop' ? !S.c.planOnly : (m === 'solo' || m === 'relay') && S.c.access === 'write';
  const tags = [
    writes ? (S.c.isolate ? '隔離實作' : '直接改檔') : '',
    S.c.verify.trim() && (m === 'solo' || m === 'relay' || (m === 'workshop' && !S.c.planOnly)) ? `驗證：${S.c.verify.trim()}` : '',
    S.c.blind && m !== 'solo' ? '盲審' : '',
    m === 'workshop' && S.c.autoApprove ? '⚠ 自動核准' : '',
  ].filter(Boolean);
  return { flow, tags };
}

function paintSummary() {
  const { flow, tags } = flowSummary();
  const el = $('#flowSummary');
  if (!el) return;
  el.innerHTML = `<span class="flow">${esc(flow)}</span>${tags.map((t) => `<span class="chip">${esc(t)}</span>`).join('')}`;
  const hint = $('#budgetHint');
  const n = (S.c.compare || []).length;
  if (hint) hint.textContent = n >= 2 ? `比較 ${n} 種模式，依序執行` : `最多 ${budgetEstimate()} 次模型呼叫`;
}

// ------------------------------------------------------------------ status, archive

function homeStatusHtml() {
  const ready = S.providers.filter((p) => p.health && p.health.ok && !p.mock).length;
  const checking = S.providers.some((p) => !p.health);
  const notReady = S.providers.filter((p) => !p.mock && p.health && !p.health.ok);
  return `<span class="status-pill"><span class="dot ${ready ? 'live' : checking ? '' : 'warn'}"></span>${checking && !ready ? '檢查 CLI 中…' : `${ready} 個 CLI 就緒`}${S.whoami?.demo ? ' · 示範模式' : ''}</span>
    ${notReady.length ? `<span class="notice">${notReady.map((p) => `${esc(p.name.split('（')[0])}：${esc(p.health.summary)}`).join('；')} <a href="#/usage">查看修復方式 ↗</a></span>` : ''}`;
}

export function refreshHomeStatus() {
  const st = $('#homeStatus');
  if (!st) return;
  st.innerHTML = homeStatusHtml();
  ensureCast();
  const cast = $('#cast');
  if (cast && !cast.contains(document.activeElement)) cast.innerHTML = castHtml();
  paintSummary();
}

function sessionCard(s, i) {
  const g = s.grade ? GRADE[s.grade] : null;
  const live = s.status === 'running' || s.status === 'awaiting_approval';
  const st = live ? `<span class="dot live"></span>${esc(s.status === 'awaiting_approval' ? '等待核准' : s.phase || '進行中')}` : esc(STATUS_ZH[s.status] || s.status);
  return `<a class="session-card${s.status === 'awaiting_approval' ? ' attention-card' : ''}" href="#/s/${esc(s.id)}">
    <span class="label">${pad2(i + 1)} / ${MODES[s.mode].en} · ${MODES[s.mode].zh}</span>
    <h3>${esc(s.title)}</h3>
    <div class="chips">${s.participants.map(chip).join('')}</div>
    <div class="card-foot"><span style="display:inline-flex;align-items:center;gap:.4rem">${st}</span><span>${g ? `<span class="eg ${g.cls}">${g.zh}</span> · ` : ''}${s.turns} 則 · ${rel(s.created_at)}</span><span aria-hidden="true">↗</span></div>
  </a>`;
}

let searchTimer;
let sessSeq = 0;
export async function loadSessions() {
  const box = $('#sessions');
  if (!box || !S.projectId) return;
  const q = ($('#sessSearch')?.value || '').trim();
  const mine = ++sessSeq;
  const project = S.projectId;
  const stale = () => mine !== sessSeq || project !== S.projectId || !$('#sessions');
  box.setAttribute('aria-busy', 'true');
  $('#sessState').textContent = '正在更新…';
  try {
    const list = await api(`/api/sessions?project=${encodeURIComponent(project)}${q ? `&q=${encodeURIComponent(q)}` : ''}`);
    if (stale()) return;
    $('#sessCount').textContent = list.length ? `${list.length} 個${q ? '符合' : ''}` : '';
    box.innerHTML = list.length ? list.map(sessionCard).join('') : `<div class="empty-state wide">${q ? '沒有符合的議題。換個關鍵字試試。' : '這個專案還沒有議題。寫下第一個問題，按「開始」。'}</div>`;
  } catch (e) {
    if (stale()) return;
    box.innerHTML = `<div class="empty-state wide">讀取失敗：${esc(e.message)}</div>`;
  } finally {
    if (mine === sessSeq) box.removeAttribute('aria-busy');
    const st = $('#sessState');
    if (st && mine === sessSeq) st.textContent = '';
  }
}

// ------------------------------------------------------------------ render

export async function renderHome() {
  const projects = S.projects;
  if (!projects.find((p) => p.id === S.projectId)) S.projectId = projects[0]?.id || null;
  ensureCast();
  const proj = projects.find((p) => p.id === S.projectId);
  const seed = S.c.seed;
  const tpl = S.templates.find((t) => t.id === S.c.template);
  const open = Boolean(store.get('adjustOpen', false));
  view().innerHTML = `
  <section class="hero compact">
    <p class="eyebrow">Multi-model workbench · 多模型工作台</p>
    <h1 class="display small" tabindex="-1">Discuss first. Then build. Verify always.</h1>
    <p class="lede">先讓 Claude 與 GPT 各自提案、互相挑錯，整合出更好的方案；你核准後才在隔離分支實作與驗證。只用你的 CLI 訂閱。</p>
    <div id="homeStatus" class="home-status">${homeStatusHtml()}</div>
  </section>

  <div id="attention">${attentionHtml()}</div>

  <section class="composer" aria-label="開新議題">
    <div class="composer-top">
      <label class="field inline"><span>專案</span><select id="projSel">${projects.map((p) => `<option value="${esc(p.id)}"${p.id === S.projectId ? ' selected' : ''}>${esc(p.name)}</option>`).join('')}<option value="__new">＋ 新增專案…</option></select></label>
      <label class="field inline"><span>從範本開始</span><select id="tplSel"><option value="">（不使用範本）</option>${S.templates.map((t) => `<option value="${esc(t.id)}"${t.id === S.c.template ? ' selected' : ''}>${esc(t.name)}${t.builtin ? '' : '（自訂）'}</option>`).join('')}</select></label>
      <span class="path" title="${esc(proj?.path || '')}">${esc(proj?.path || '')}</span>
    </div>
    <form id="newProj" class="options" hidden>
      <label class="field"><span>名稱</span><input type="text" name="name" required placeholder="例如 Agora"></label>
      <label class="field"><span>資料夾路徑（CLI 的工作目錄）</span><input type="text" name="path" required placeholder="D:\\Github repo\\Agora"></label>
      <div class="field" style="align-self:end"><button class="btn primary" type="submit">建立</button></div>
    </form>
    ${tpl ? `<p class="tpl-note"><b>${esc(tpl.name)}</b>：${esc(tpl.summary)}</p>` : ''}

    <div class="mode-seg" role="radiogroup" aria-label="協作方式" id="modes">${MODE_ORDER.map((m) => `<button type="button" role="radio" aria-checked="${S.c.mode === m}" tabindex="${S.c.mode === m ? 0 : -1}" class="${S.c.mode === m ? 'on' : ''}${m === 'workshop' ? ' core' : ''}" data-mode="${m}">${esc(SHORT[m])}</button>`).join('')}</div>
    <p class="mode-desc" id="modeDesc">${esc(MODES[S.c.mode].desc)}</p>

    ${seed ? `<div class="banner seed"><b>分岔</b><span>會延續「${esc(seed.title)}」第 1–${seed.upto + 1} 則發言，再接著進行。</span><button class="btn small quiet" type="button" id="dropSeed">取消分岔</button></div>` : ''}
    <label class="field"><span class="sr">議題內容</span><textarea id="prompt" class="prompt" placeholder="要做什麼、討論什麼、辯論什麼？例：幫這個專案加上匯出 CSV 功能；或：這個服務該用 SQLite 還是 Postgres？">${esc(S.c.prompt)}</textarea></label>

    <div class="flow-row">
      <div class="flow-summary" id="flowSummary" aria-live="polite"></div>
      <button class="btn small quiet" type="button" id="adjustBtn" aria-expanded="${open}" aria-controls="adjust">${open ? '收起 ▴' : '調整 ▾'}</button>
    </div>

    <div id="adjust" class="adjust" ${open ? '' : 'hidden'}>
      <h3 class="sub">參與者</h3>
      <div class="cast" id="cast">${castHtml()}</div>
      <h3 class="sub">選項</h3>
      <div class="options" id="opts">${optionsHtml()}</div>
      ${compareHtml()}
      ${proj ? `<details class="brief memory-box"><summary>專案記憶（${proj.memory.trim() ? proj.memory.trim().split('\n').length + ' 行' : '空白'}）— 每次發言都會帶入；超過 3000 字時依議題檢索最相關的部分</summary>
        <textarea id="memoryText" placeholder="決策、慣例、教訓……議題結束後可以按「存入專案記憶」自動加一行">${esc(proj.memory)}</textarea>
        <div class="toolbar" style="margin-top:.6rem"><button class="btn small" type="button" id="saveMemory">儲存記憶</button></div></details>` : ''}
    </div>

    <div class="composer-foot">
      <button class="btn primary" type="button" id="startBtn">開始 ↗</button>
      <span class="hint" id="budgetHint"></span><span class="hint">· Ctrl + Enter</span>
      <span class="spacer"></span>
      ${voiceButton('prompt')}
      <button class="btn small quiet" type="button" id="saveTemplate">存成範本</button>
    </div>
  </section>

  <section class="block" id="archive">
    <div class="block-head"><span class="index">Archive</span><h2>議題紀錄</h2><span class="aside"><span id="sessState" class="updating" aria-live="polite"></span> <span id="sessCount"></span></span></div>
    <input type="search" id="sessSearch" class="search" placeholder="搜尋議題標題、內容或任何發言…" aria-label="搜尋議題">
    <div class="session-grid" id="sessions"></div>
  </section>`;
  paintSummary();
  bindHome();
  bindVoice(view());
  loadSessions();
  refreshAttention();
}

function refreshComposer() {
  $('#cast').innerHTML = castHtml();
  $('#opts').innerHTML = optionsHtml();
  $('#modeDesc').textContent = MODES[S.c.mode].desc;
  $$('#modes [data-mode]').forEach((b) => {
    const on = b.dataset.mode === S.c.mode;
    b.classList.toggle('on', on);
    b.setAttribute('aria-checked', on);
    b.tabIndex = on ? 0 : -1;
  });
  paintSummary();
  saveComposer();
}

function applyTemplate(t) {
  const prompt = $('#prompt');
  const current = prompt.value.trim();
  const previousScaffold = (S.templates.find((x) => x.id === S.c.template)?.scaffold || '').trim();
  if (current && current !== previousScaffold && !confirm('套用範本會取代目前的議題內容，確定嗎？')) {
    $('#tplSel').value = S.c.template || '';
    return;
  }
  S.c.mode = t.mode;
  if (t.rounds != null) S.c.rounds[t.mode] = t.rounds;
  S.c.access = t.access || 'read_only';
  S.c.blind = Boolean(t.blind);
  S.c.redTeam = Boolean(t.red_team);
  S.c.planOnly = Boolean(t.plan_only);
  S.c.verifyHint = t.verify_hint || '';
  S.c.template = t.id;
  S.c.prompt = t.scaffold;
  saveComposer();
  renderHome().then(() => {
    const p = $('#prompt');
    const at = p.value.indexOf('\n') + 1 || p.value.length;
    p.focus();
    p.setSelectionRange(at, at);
  });
  toast(`已套用範本「${t.name}」`);
}

function setMode(m) {
  S.c.mode = m;
  if (m !== 'solo' && m !== 'relay') S.c.access = 'read_only';
  refreshComposer();
}

function bindHome() {
  $('#projSel').addEventListener('change', (e) => {
    if (e.target.value === '__new') {
      const f = $('#newProj');
      f.hidden = false;
      f.name.focus();
      e.target.value = S.projectId;
      return;
    }
    S.projectId = e.target.value;
    store.set('project', S.projectId);
    renderHome();
  });
  $('#tplSel').addEventListener('change', (e) => {
    const t = S.templates.find((x) => x.id === e.target.value);
    if (t) applyTemplate(t);
    else { S.c.template = null; saveComposer(); renderHome(); }
  });
  $('#newProj').addEventListener('submit', async (e) => {
    e.preventDefault();
    const f = e.target;
    try {
      const p = await api('/api/projects', { method: 'POST', body: { name: f.name.value, path: f.path.value } });
      S.projects = await api('/api/projects');
      S.projectId = p.id; store.set('project', p.id);
      toast(`已建立專案「${p.name}」`);
      renderHome();
    } catch (err) { toast(err.message, true); }
  });
  const sm = $('#saveMemory');
  if (sm) sm.addEventListener('click', async () => {
    try {
      const p = await api(`/api/projects/${encodeURIComponent(S.projectId)}`, { method: 'PATCH', body: { memory: $('#memoryText').value } });
      S.projects = S.projects.map((x) => (x.id === p.id ? p : x));
      toast('專案記憶已儲存');
    } catch (err) { toast(err.message, true); }
  });
  const modes = $('#modes');
  modes.addEventListener('click', (e) => {
    const b = e.target.closest('[data-mode]');
    if (b) setMode(b.dataset.mode);
  });
  modes.addEventListener('keydown', (e) => {
    if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(e.key)) return;
    e.preventDefault();
    const i = MODE_ORDER.indexOf(S.c.mode);
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? MODE_ORDER.length - 1
      : (i + (e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 1) + MODE_ORDER.length) % MODE_ORDER.length;
    setMode(MODE_ORDER[next]);
    modes.querySelector(`[data-mode="${MODE_ORDER[next]}"]`).focus();
  });
  $('#adjustBtn').addEventListener('click', (e) => {
    const box = $('#adjust');
    box.hidden = !box.hidden;
    e.currentTarget.setAttribute('aria-expanded', String(!box.hidden));
    e.currentTarget.textContent = box.hidden ? '調整 ▾' : '收起 ▴';
    store.set('adjustOpen', !box.hidden);
  });
  $('#cast').addEventListener('change', (e) => {
    const slots = S.c.cast[S.c.mode];
    const t = e.target;
    if (t.dataset.prov !== undefined) { const s = slots[Number(t.dataset.prov)]; s.provider = t.value; s.model = ''; refreshComposer(); return; }
    if (t.dataset.model !== undefined) { slots[Number(t.dataset.model)].model = t.value; saveComposer(); return; }
    if (t.id === 'judgeSel') S.c.judge[S.c.mode] = t.value;
    if (t.id === 'builderSel') S.c.builder = t.value;
    if (t.id === 'reviewerSel') S.c.reviewer = t.value;
    paintSummary();
    saveComposer();
  });
  $('#cast').addEventListener('click', (e) => {
    const slots = S.c.cast[S.c.mode];
    if (e.target.closest('#addSlot')) {
      const used = slots.map((s) => famOf(s.provider));
      const fam = used.filter((f) => f === 'claude').length <= used.filter((f) => f === 'gpt').length ? 'claude' : 'gpt';
      slots.push({ provider: pickProvider(fam), model: '' });
      refreshComposer();
    }
    const rm = e.target.closest('[data-remove]');
    if (rm) { slots.splice(Number(rm.dataset.remove), 1); refreshComposer(); }
  });
  const opts = $('#opts');
  opts.addEventListener('click', (e) => {
    const b = e.target.closest('[data-access]');
    if (!b) return;
    S.c.access = b.dataset.access;
    refreshComposer();
  });
  opts.addEventListener('change', (e) => {
    const t = e.target;
    const map = { optRed: 'redTeam', optPlanOnly: 'planOnly', optAuto: 'autoApprove', optIsolate: 'isolate', optBlind: 'blind' };
    if (!map[t.id]) return;
    S.c[map[t.id]] = t.checked;
    if (t.id === 'optAuto' && t.checked) toast('自動核准會跳過你檢查方案的機會；建議保持關閉。');
    if (t.id === 'optPlanOnly') { refreshComposer(); return; }
    paintSummary();
    saveComposer();
  });
  opts.addEventListener('input', (e) => {
    const t = e.target;
    if (t.id === 'optRounds') S.c.rounds[S.c.mode] = Number(t.value);
    if (t.id === 'optVerify') S.c.verify = t.value;
    if (t.id === 'optMax') S.c.maxTurns = t.value;
    paintSummary();
    saveComposer();
  });
  $('.compare').addEventListener('change', (e) => {
    const m = e.target.id?.startsWith('cmp-') ? e.target.id.slice(4) : null;
    if (!m) return;
    S.c.compare = e.target.checked ? [...new Set([...(S.c.compare || []), m])] : (S.c.compare || []).filter((x) => x !== m);
    paintSummary();
    saveComposer();
  });
  const ds = $('#dropSeed');
  if (ds) ds.onclick = () => { S.c.seed = null; saveComposer(); renderHome(); };
  $('#prompt').addEventListener('input', (e) => { S.c.prompt = e.target.value; saveComposer(); });
  $('#prompt').addEventListener('keydown', (e) => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && !e.isComposing) { e.preventDefault(); startSession(); } });
  $('#startBtn').addEventListener('click', startSession);
  $('#saveTemplate').addEventListener('click', saveAsTemplate);
  $('#sessSearch').addEventListener('input', () => { clearTimeout(searchTimer); searchTimer = setTimeout(loadSessions, 250); });
}

export function buildSpec() {
  const mode = S.c.mode;
  const prompt = ($('#prompt')?.value ?? S.c.prompt).trim();
  const one = (id) => (id ? { provider: id, model: null } : null);
  const writes = mode === 'workshop' ? !S.c.planOnly : (mode === 'solo' || mode === 'relay') && S.c.access === 'write';
  return {
    project_id: S.projectId,
    mode,
    participants: S.c.cast[mode].map((s) => ({ provider: s.provider, model: s.model || null })),
    judge: ['roundtable', 'debate', 'workshop'].includes(mode) ? one(S.c.judge[mode]) : null,
    builder: mode === 'workshop' ? one(S.c.builder) : null,
    reviewer: mode === 'workshop' ? one(S.c.reviewer) : null,
    prompt,
    rounds: mode === 'solo' ? null : Number(S.c.rounds[mode]),
    access: mode === 'solo' || mode === 'relay' ? S.c.access : mode === 'workshop' && !S.c.planOnly ? 'write' : 'read_only',
    verify_command: (mode === 'solo' || mode === 'relay' || (mode === 'workshop' && !S.c.planOnly)) && S.c.verify.trim() ? S.c.verify.trim() : null,
    max_turns: Number(S.c.maxTurns) > 0 ? Number(S.c.maxTurns) : null,
    blind: mode !== 'solo' && S.c.blind,
    red_team: mode === 'workshop' && S.c.redTeam,
    plan_only: mode === 'workshop' && S.c.planOnly,
    auto_approve: mode === 'workshop' && S.c.autoApprove,
    isolate: writes ? S.c.isolate : true,
    seed: S.c.seed ? { session_id: S.c.seed.session_id, upto: S.c.seed.upto } : null,
    template: S.c.template,
  };
}

let starting = false;
async function startSession() {
  if (starting) return;
  const btn = $('#startBtn');
  const spec = buildSpec();
  if (!spec.prompt) { toast('請先輸入議題內容', true); $('#prompt')?.focus(); return; }
  const compare = S.c.compare || [];
  if (compare.length === 1) { toast('同題比較至少要勾選 2 種模式（或全部取消）', true); return; }
  if (compare.length < 2 && spec.access === 'write' && !spec.isolate && !confirm('這個議題會直接修改專案資料夾裡的檔案（沒有隔離）。確定要開始嗎？')) return;
  starting = true;
  btn.disabled = true;
  try {
    if (compare.length >= 2) {
      const x = await api('/api/experiments', { method: 'POST', body: { spec: { ...spec, seed: null }, modes: compare } });
      location.hash = `#/x/${x.id}`;
    } else {
      const rec = await api('/api/sessions', { method: 'POST', body: spec });
      location.hash = `#/s/${rec.id}`;
    }
    S.c.prompt = '';
    S.c.seed = null;
    S.c.template = null;
    saveComposer();
  } catch (e) {
    toast(e.message, true);
  } finally {
    starting = false;
    btn.disabled = false;
  }
}

async function saveAsTemplate() {
  const name = window.prompt('範本名稱：');
  if (!name || !name.trim()) return;
  const spec = buildSpec();
  try {
    const t = await api('/api/templates', {
      method: 'POST',
      body: {
        id: '', name: name.trim(), summary: `自訂：${MODES[spec.mode].zh}`, mode: spec.mode, rounds: spec.rounds, access: spec.access,
        scaffold: spec.prompt || '', verify_hint: spec.verify_command, blind: spec.blind, red_team: spec.red_team, plan_only: spec.plan_only, adr: false, source: null, builtin: false,
      },
    });
    S.templates = await api('/api/templates');
    toast(`已存成範本「${t.name}」`);
    renderHome();
  } catch (e) { toast(e.message, true); }
}
