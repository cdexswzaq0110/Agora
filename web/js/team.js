// Project team and workflow views (v0.5).
import { $, $$, api, esc, nextFrame, rel, store, toast } from './util.js';
import { md } from './md.js';
import { S, FAMILY, pickProvider } from './state.js';
import { ping } from './motion.js';

const view = () => $('#view');
const STAGE_STATUS = { pending: '尚未開始', running: '進行中', awaiting_approval: '等你核准', approved: '已核准', failed: '失敗，可重跑', rejected: '未採用', stale: '已過期，需重跑' };
const WORKFLOW_STATUS = { active: '進行中', stopped: '已停止', done: '已完成' };
const CHECK_STATUS = { full: '完整', brief: '簡查', not_applicable: '不適用', missing: '缺漏' };
let teamInfo = null;
let workflowId = null;
let refreshQueued = false;
let pingedRender = false;

export async function loadTeamInfo() {
  if (!teamInfo) teamInfo = S.teamInfo || await api('/api/team');
  S.teamInfo = teamInfo;
  return teamInfo;
}

const projectOptions = () => S.projects.map((p) => `<option value="${esc(p.id)}"${p.id === S.projectId ? ' selected' : ''}>${esc(p.name)}</option>`).join('');
const stageLabel = (kind) => teamInfo?.stages.find((s) => s.kind === kind)?.label || kind || '—';
const stageStatus = (status) => STAGE_STATUS[status] || status;
const workflowStatus = (status) => WORKFLOW_STATUS[status] || status;
const planCalls = (plan) => (plan?.producers?.length || 0) + (plan?.integrator ? 1 : 0);
const dateValue = (value) => value && value < 1e12 ? value * 1000 : value;

// One line per stage for the home-page pipeline (what the team hands you at each gate).
const STAGE_GOALS = {
  discovery: '問題、目標使用者、範圍、成功指標；列出需要你回答的問題',
  feasibility: '技術、資料、AI、時程、成本、風險與安全；建議做／調整／不做',
  product: 'PRD：使用者故事與驗收標準、UX 流程、非功能需求',
  design: '架構、元件與 API、資料模型、AI／ML 設計、安全、ADR',
  tasks: '依開發順序的任務，每項附驗收標準與驗證指令，可一鍵交給工作坊',
};
const roleName = (id) => teamInfo.roles.find((r) => r.id === id)?.name || id;

/** All 18 roles, always visible, grouped. */
function rosterPanel() {
  const groups = [...new Set(teamInfo.roles.map((r) => r.group))];
  return groups.map((group) => {
    const roles = teamInfo.roles.filter((r) => r.group === group);
    return `<div class="roster-row"><h3>${esc(roles[0].group_label)}</h3><ul>${roles.map((r) => `<li title="${esc(r.duty)}"><b>${esc(r.name)}</b><small>${esc(r.title)}</small></li>`).join('')}</ul></div>`;
  }).join('');
}

/** Who plays which role at each stage, for the depth and kind you picked. */
function pipeline(depth, kind) {
  const plans = teamInfo.depths[depth]?.plans?.[kind] || [];
  return `<ol class="team-pipeline">${teamInfo.stages.map((stage, i) => {
    const plan = plans[i];
    const calls = planCalls(plan);
    const panels = [...(plan?.producers || []).map((p) => ['', p]), ...(plan?.integrator ? [['審查與整合', plan.integrator]] : [])];
    const panelHtml = panels.map(([tag, p]) => `<li class="fam-${esc(p.family)}"><span>${tag ? `${esc(tag)} · ` : ''}${esc(FAMILY[p.family] || p.family)}</span>${p.roles.length ? p.roles.map((id) => `<em>${esc(roleName(id))}</em>`).join('') : '<em>交叉審查所有產出</em>'}</li>`).join('');
    return `<li><span class="stage-number">${String(i + 1).padStart(2, '0')}</span><h3>${esc(stage.label)}</h3><p>${esc(STAGE_GOALS[stage.kind] || '')}</p><ul class="pipeline-panels">${panelHtml}</ul><small class="call-note">基本 ${calls} 次／最多 ${calls + 2} 次呼叫・等你核准</small></li>`;
  }).join('')}</ol>`;
}

function workflowCard(w) {
  const approved = w.stages.filter((s) => s.status === 'approved').length;
  const awaiting = w.stages.some((s) => s.status === 'awaiting_approval');
  const next = awaiting ? w.stages.find((s) => s.status === 'awaiting_approval')?.kind : w.next;
  return `<a class="workflow-card${awaiting ? ' needs-you' : ''}" href="#/w/${esc(w.id)}">
    <div><span class="status-shape status-${esc(w.status)}"></span><span>${esc(workflowStatus(w.status))}</span>${awaiting ? '<strong>等你核准</strong>' : ''}</div>
    <h3>${esc(w.title || '未命名工作流')}</h3>
    <dl><div><dt>下一步</dt><dd>${esc(next ? stageLabel(next) : w.status === 'done' ? '交付任務' : '—')}</dd></div><div><dt>已核准</dt><dd>${approved}／5</dd></div><div><dt>更新</dt><dd>${esc(rel(dateValue(w.updated_at)))}</dd></div></dl>
  </a>`;
}

async function loadWorkflowList() {
  const box = $('#workflowList');
  if (!box || !S.projectId) return;
  try {
    const list = await api(`/api/workflows?project_id=${encodeURIComponent(S.projectId)}`);
    if (!$('#workflowList')) return;
    box.innerHTML = list.length ? list.map(workflowCard).join('') : '<div class="empty-state wide">這個程式庫還沒有工作流。從上方寫下第一個構想。</div>';
  } catch (e) { box.innerHTML = `<div class="empty-state wide">讀取失敗：${esc(e.message)}</div>`; }
}

function depthOptions(kind = 'general', selected = 'standard') {
  // Fixed order: the server's JSON map is sorted alphabetically (full, quick, standard).
  return ['quick', 'standard', 'full'].filter((id) => teamInfo.depths[id]).map((id) => {
    const estimate = teamInfo.depths[id][kind];
    return `<label class="depth-option"><input type="radio" name="depth" value="${esc(id)}"${id === selected ? ' checked' : ''}><span><b>${esc(teamInfo.depths[id].label)}</b><small>基本 ${estimate.calls} 次／含修訂最多 ${estimate.cap} 次</small></span></label>`;
  }).join('');
}

/** Home page (v0.5): your idea → the 18-role team → five gated stages. */
export async function renderTeam() {
  workflowId = null;
  await loadTeamInfo();
  if (!S.projects.some((p) => p.id === S.projectId)) S.projectId = S.projects[0]?.id || null;
  view().innerHTML = `<section class="hero team-hero"><p class="eyebrow">Project Team · 你的專案團隊</p><h1 class="display zh">你的構想，交給 18 人團隊</h1><p class="lede">寫下構想，PM、UX、SA、Architect、SD、DBA、Dev、QA、DevOps/SRE、系統設計師、系統架構師、資料分析師與 AI 小組（LLM、NLP、AI、ML、MLOps、DL）依序完成構想收斂、可行性分析、需求與體驗、技術設計、任務拆分。每個階段都停下來等你核准。</p></section>
    <div class="team-home">
      <section class="team-composer block" aria-labelledby="newIdeaTitle"><div class="block-head"><span class="index">01 / IDEA</span><h2 id="newIdeaTitle">新構想</h2></div>
        <form id="ideaForm"><div class="team-form-grid">
          <label class="field"><span>程式庫</span><select id="teamProject" required>${projectOptions()}</select></label>
          <label class="field"><span>標題（選填）</span><input id="ideaTitle" name="title" type="text" maxlength="120" placeholder="例如：離線優先的專案筆記"></label>
        </div>
        <label class="field idea-field"><span>構想</span><textarea id="ideaText" name="idea" required maxlength="${teamInfo.max_idea_chars}" placeholder="想解決什麼問題？給誰用？有哪些限制、已知條件、想用或不想用的技術？寫得越具體，可行性與技術分析越準。"></textarea><small id="ideaCount">0／${teamInfo.max_idea_chars}</small></label>
        <fieldset class="team-choice"><legend>類型</legend><label><input type="radio" name="kind" value="general" checked><span><b>一般</b><small>AI 小組在可行性階段一起審查</small></span></label><label><input type="radio" name="kind" value="ai"><span><b>AI 類</b><small>AI 小組獨立成一次呼叫，完整審視模型與資料</small></span></label></fieldset>
        <fieldset class="team-choice depth-choice"><legend>深度</legend><div id="depthOptions">${depthOptions()}</div></fieldset>
        <div class="composer-foot"><button class="btn primary" type="submit" id="createWorkflow" data-magnetic>建立工作流 ↗</button><span class="hint">建立後從「構想收斂」開始，每個階段都由你核准。</span></div></form>
      </section>
      <aside class="team-roster block" aria-labelledby="rosterTitle"><div class="block-head"><span class="index">TEAM / 18</span><h2 id="rosterTitle">你的團隊</h2></div>${rosterPanel()}<p class="team-methods"><a href="#/skills">方法（skills）→</a></p></aside>
    </div>
    <section class="block"><div class="block-head"><span class="index">02 / WORK</span><h2>這個程式庫的工作流</h2></div><div id="workflowList" class="workflow-list" aria-live="polite"><div class="empty-state wide">載入中…</div></div></section>
    <section class="block" aria-labelledby="pipelineTitle"><div class="block-head"><span class="index">03 / FLOW</span><h2 id="pipelineTitle">這個構想會怎麼跑</h2><span class="aside">依你選的類型與深度</span></div><div id="teamPipeline">${pipeline('standard', 'general')}</div></section>`;
  bindTeam();
  loadWorkflowList();
}

function bindTeam() {
  const choice = (name) => $(`input[name="${name}"]:checked`)?.value;
  const refreshPlan = () => { $('#teamPipeline').innerHTML = pipeline(choice('depth') || 'standard', choice('kind') || 'general'); };
  $('#teamProject').addEventListener('change', (e) => { S.projectId = e.target.value; store.set('project', S.projectId); loadWorkflowList(); });
  $('#ideaText').addEventListener('input', (e) => { $('#ideaCount').textContent = `${e.target.value.length}／${teamInfo.max_idea_chars}`; });
  $$('input[name="kind"]').forEach((input) => input.addEventListener('change', (e) => {
    // Keep the depth you picked; only the estimates change with the kind.
    $('#depthOptions').innerHTML = depthOptions(e.target.value, choice('depth') || 'standard');
    refreshPlan();
  }));
  $('#depthOptions').addEventListener('change', refreshPlan);
  $('#ideaForm').addEventListener('submit', async (e) => {
    e.preventDefault();
    const button = $('#createWorkflow');
    const idea = $('#ideaText').value.trim();
    if (!idea) { toast('請先輸入構想', true); $('#ideaText').focus(); return; }
    button.disabled = true;
    try {
      const body = { project_id: S.projectId, idea, depth: choice('depth'), kind: choice('kind') };
      const title = $('#ideaTitle').value.trim();
      if (title) body.title = title;
      const claude = pickProvider('claude'), gpt = pickProvider('gpt');
      if (claude) body.claude = claude;
      if (gpt) body.gpt = gpt;
      const workflow = await api('/api/workflows', { method: 'POST', body });
      location.hash = `#/w/${workflow.id}`;
    } catch (err) { toast(err.message, true); button.disabled = false; }
  });
}

function stageTrack(w) {
  return `<ol class="stage-track" aria-label="工作流階段">${w.stages.map((stage, i) => {
    const linked = Boolean(stage.session_id);
    const inner = `<span class="stage-number">${String(i + 1).padStart(2, '0')}</span><b>${esc(stageLabel(stage.kind))}</b><span class="stage-state"><i class="status-shape stage-${esc(stage.status)}"></i>${esc(stageStatus(stage.status))}</span><small>${stage.revision ? `r${stage.revision}` : '—'}</small>`;
    return `<li class="stage-${esc(stage.status)}"${w.next === stage.kind ? ' aria-current="step"' : ''}>${i ? `<svg class="stage-line${stage.status === 'running' ? ' net-pulse' : ''}" viewBox="0 0 32 4" aria-hidden="true"><path d="M0 2h32"/></svg>` : ''}${linked ? `<a href="#/s/${esc(stage.session_id)}">${inner}</a>` : `<div>${inner}</div>`}</li>`;
  }).join('')}</ol>`;
}

function panelText(panel) {
  if (!panel) return '';
  return `${FAMILY[panel.family] || panel.family}：${panel.roles.map((id) => teamInfo.roles.find((r) => r.id === id)?.name || id).join('、')}`;
}

function nextCard(w) {
  const awaiting = w.stages.find((s) => s.status === 'awaiting_approval');
  const running = w.stages.find((s) => s.status === 'running');
  if (w.status === 'stopped') return '<section class="next-card stopped"><p class="eyebrow">Next · 下一步</p><h2>可行性分析決定不做</h2><p>工作流已停止，已產生的文件仍完整保存。</p></section>';
  if (w.status === 'done') return '<section class="next-card done"><p class="eyebrow">Next · 下一步</p><h2>全部完成</h2><p>文件已核准，可以把任務交給工作坊。</p><a class="btn primary" href="#workflowTasks" data-jump-tasks>查看任務 ↓</a></section>';
  const stage = awaiting || running || w.stages.find((s) => s.kind === w.next);
  if (!stage) return '';
  const label = stageLabel(stage.kind);
  if (awaiting) return `<section class="next-card attention-card"><p class="eyebrow">Next · 等你決定</p><h2>核准「${esc(label)}」</h2><p>團隊已整理好本階段文件，核准或提出修訂後才會往下走。</p><a class="btn primary" href="#/s/${esc(stage.session_id)}">去核准「${esc(label)}」↗</a></section>`;
  if (running) return `<section class="next-card"><p class="eyebrow">Live · 進行中</p><h2>「${esc(label)}」進行中</h2><p>團隊正在依角色分工產出文件，你可以查看即時發言。</p><a class="btn" href="#/s/${esc(stage.session_id)}">查看即時發言 ↗</a></section>`;
  const plan = w.plans[w.stages.findIndex((s) => s.kind === stage.kind)];
  const calls = planCalls(plan);
  const actors = [...(plan?.producers || []).map(panelText), ...(plan?.integrator ? [`整合 · ${panelText(plan.integrator)}`] : [])];
  return `<section class="next-card${stage.status === 'stale' ? ' stale' : ''}"><p class="eyebrow">Next · 下一步</p><h2>${stage.status === 'stale' ? '重新開始' : '開始'}「${esc(label)}」</h2>${stage.status === 'stale' ? '<p class="warning">上游文件已有新版本，這一階段的舊結果已過期，需要重跑。</p>' : ''}<ul>${actors.map((x) => `<li>${esc(x)}</li>`).join('')}</ul><p class="call-note">基本 ${calls} 次模型呼叫／含修訂最多 ${calls + 2} 次</p><button class="btn primary" type="button" data-start-stage="${esc(stage.kind)}" data-magnetic>開始「${esc(label)}」↗</button></section>`;
}

function roleAssignment(plan, roleId) {
  const producers = plan?.producers || [];
  for (const panel of producers) if (panel.roles.includes(roleId)) return FAMILY[panel.family] || panel.family;
  if (plan?.integrator?.roles.includes(roleId)) return `${FAMILY[plan.integrator.family] || plan.integrator.family}（整合）`;
  return '本階段不參與';
}

function latestCheck(w, roleId) {
  for (let i = w.stages.length - 1; i >= 0; i--) {
    const check = (w.stages[i].role_checks || []).find((x) => x.role === roleId);
    if (check) return { ...check, stage: w.stages[i].kind };
  }
  return null;
}

function roster(w) {
  const current = w.stages.find((s) => ['running', 'awaiting_approval'].includes(s.status)) || w.stages.find((s) => s.kind === w.next);
  const index = current ? w.stages.findIndex((s) => s.kind === current.kind) : -1;
  const plan = index >= 0 ? w.plans[index] : null;
  const groups = [...new Set(teamInfo.roles.map((r) => r.group))];
  return groups.map((group) => {
    const roles = teamInfo.roles.filter((r) => r.group === group);
    return `<section class="roster-group"><h3>${esc(roles[0].group_label)}</h3><div class="role-grid">${roles.map((role) => {
      const assigned = roleAssignment(plan, role.id);
      const check = latestCheck(w, role.id);
      const active = current?.status === 'running' && assigned !== '本階段不參與';
      return `<article class="role-card${active ? ' role-active' : ''}" data-active-role="${active ? '1' : '0'}"><header><h4>${esc(role.name)}</h4><span>${esc(role.title)}</span></header>${current ? `<p class="role-owner">${esc(stageLabel(current.kind))} · <b>${esc(assigned)}</b></p>` : ''}${check ? `<div class="role-check check-${esc(check.status)}"><span>${esc(CHECK_STATUS[check.status] || check.status)}${check.status === 'missing' ? ' ⚠' : ''}</span><small>${esc(stageLabel(check.stage))}</small></div><p class="role-summary">${esc(check.summary || (check.status === 'missing' ? '沒有找到這個角色的有效紀錄。' : '—'))}</p>` : '<p class="role-summary muted">尚無階段紀錄</p>'}</article>`;
    }).join('')}</div></section>`;
  }).join('');
}

function artifacts(w) {
  const rows = w.stages.filter((s) => s.artifact);
  return rows.length ? rows.map((s) => `<details class="artifact" data-detail="${esc(s.kind)}"><summary><b>${esc(stageLabel(s.kind))}</b><span>r${s.revision} · ${esc(stageStatus(s.status))}${s.based_on?.length ? ` · 基於 ${s.based_on.map((n) => `r${n}`).join('／')}` : ''}</span></summary><div class="md">${md(s.artifact)}</div></details>`).join('') : '<p class="muted">核准後，階段文件會出現在這裡。</p>';
}

function tasks(w) {
  if (!w.tasks.length) return '<p class="muted">「任務拆分」核准後會產生任務。</p>';
  return w.tasks.map((task) => `<article class="workflow-task"><div><h3>${esc(task.title)}</h3><div class="md">${md(task.body)}</div></div>${task.session_id ? `<a class="btn" href="#/s/${esc(task.session_id)}">開啟專案 ↗</a>` : `<button class="btn" type="button" data-start-task="${esc(task.id)}">交給工作坊 ↗</button>`}</article>`).join('');
}

function preserveView() {
  const active = document.activeElement;
  const focus = active?.id ? `#${CSS.escape(active.id)}`
    : active?.dataset?.startStage ? `[data-start-stage="${CSS.escape(active.dataset.startStage)}"]`
      : active?.dataset?.startTask ? `[data-start-task="${CSS.escape(active.dataset.startTask)}"]` : null;
  return { scroll: window.scrollY, focus, open: new Set($$('details[data-detail][open]').map((d) => d.dataset.detail)) };
}

export async function renderWorkflow(id, { live = false } = {}) {
  workflowId = id;
  await loadTeamInfo();
  const saved = live ? preserveView() : { scroll: 0, focus: null, open: new Set() };
  let w;
  try { w = await api(`/api/workflows/${encodeURIComponent(id)}`); }
  catch (e) { view().innerHTML = `<section class="hero"><h1 class="display zh">找不到工作流</h1><p class="lede">${esc(e.message)}</p><a class="btn" href="#/">返回團隊</a></section>`; return; }
  if (workflowId !== id) return;
  pingedRender = false;
  const percent = Math.min(100, w.cap ? Math.round(w.calls_used / w.cap * 100) : 0);
  view().innerHTML = `<header class="workflow-head"><a class="back" href="#/">← 專案團隊</a><p class="eyebrow">${esc(teamInfo.depths[w.depth]?.label || w.depth)} · ${w.kind === 'ai' ? 'AI 類' : '一般'}專案</p><div class="workflow-title-row"><h1 class="display zh">${esc(w.title || '未命名工作流')}</h1><span class="workflow-status status-${esc(w.status)}"><i class="status-shape"></i>${esc(workflowStatus(w.status))}</span></div><div class="call-meter"><div><span>模型呼叫用量</span><strong>${w.calls_used}／${w.cap}</strong></div><progress max="100" value="${percent}">${percent}%</progress></div></header>
    <section class="workflow-above">${stageTrack(w)}${nextCard(w)}</section>
    <section class="block workflow-roster"><div class="block-head"><span class="index">Team / 18</span><h2>團隊名冊</h2><span class="aside">目前／下一階段的角色分工與最近紀錄</span></div>${roster(w)}</section>
    <section class="block"><div class="block-head"><span class="index">Docs</span><h2>已核准的文件</h2></div>${artifacts(w)}</section>
    <section class="block" id="workflowTasks"><div class="block-head"><span class="index">Tasks</span><h2>任務清單</h2></div><div class="workflow-tasks">${tasks(w)}</div></section>
    ${S.whoami?.local ? '<section class="workflow-export"><button class="btn quiet" type="button" id="exportWorkflow">匯出到程式庫（docs/agora/…）</button></section>' : ''}`;
  saved.open.forEach((key) => { const d = $(`details[data-detail="${CSS.escape(key)}"]`); if (d) d.open = true; });
  bindWorkflow(w);
  if (live) {
    // Restore synchronously: the new DOM is already in place, so there is no frame to wait for.
    window.scrollTo({ top: saved.scroll });
    if (saved.focus) $(saved.focus)?.focus({ preventScroll: true });
  }
  const active = $('[data-active-role="1"]');
  if (active && !pingedRender) { pingedRender = true; ping(active); }
}

async function startStage(w, kind, raiseCap = false) {
  try {
    const session = await api(`/api/workflows/${encodeURIComponent(w.id)}/stages/${encodeURIComponent(kind)}/start`, { method: 'POST', body: raiseCap ? { raise_cap: true } : {} });
    location.hash = `#/s/${session.id}`;
  } catch (e) {
    if (e.status === 409 && !raiseCap && confirm(`${e.message}\n\n要提高這個工作流的呼叫上限並繼續嗎？`)) { await startStage(w, kind, true); return; }
    toast(e.message, true);
    const button = $(`[data-start-stage="${CSS.escape(kind)}"]`);
    if (button) button.disabled = false;
  }
}

function bindWorkflow(w) {
  $('[data-start-stage]')?.addEventListener('click', (e) => { e.currentTarget.disabled = true; startStage(w, e.currentTarget.dataset.startStage); });
  $$('[data-start-task]').forEach((button) => button.addEventListener('click', async () => {
    button.disabled = true;
    try { const session = await api(`/api/workflows/${encodeURIComponent(w.id)}/tasks/${encodeURIComponent(button.dataset.startTask)}/start`, { method: 'POST', body: {} }); location.hash = `#/s/${session.id}`; }
    catch (e) { toast(e.message, true); button.disabled = false; }
  }));
  $('[data-jump-tasks]')?.addEventListener('click', (e) => { e.preventDefault(); $('#workflowTasks')?.scrollIntoView({ behavior: 'smooth' }); });
  $('#exportWorkflow')?.addEventListener('click', async (e) => {
    const button = e.currentTarget; // currentTarget is null once the handler awaits
    button.disabled = true;
    try { const result = await api(`/api/workflows/${encodeURIComponent(w.id)}/export`, { method: 'POST', body: {} }); toast(`已寫入 ${result.paths.length} 個檔案${result.paths[0] ? `：${result.paths[0]}` : ''}`); }
    catch (err) { toast(err.message, true); }
    finally { button.disabled = false; }
  });
}

export function onWorkflowEvent(event) {
  const hash = location.hash || '#/';
  const onWorkflow = workflowId && hash === `#/w/${workflowId}` && event.workflow_id === workflowId;
  const onTeam = (hash === '#/' || hash.startsWith('#/team')) && $('#workflowList');
  if (!onWorkflow && !onTeam) return;
  if (refreshQueued) return;
  refreshQueued = true;
  // nextFrame, not raw rAF: a frame that never fires would drop every later event (lessons/0005).
  nextFrame(() => {
    refreshQueued = false;
    const current = location.hash || '#/';
    if (onWorkflow && current === `#/w/${workflowId}`) renderWorkflow(workflowId, { live: true });
    else if (onTeam && (current === '#/' || current.startsWith('#/team'))) loadWorkflowList();
  });
}
