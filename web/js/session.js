// Session view (v0.3): outcome first, discussion folded by round with one-line summaries,
// phase index in the rail, approval gate as "plan document + sticky decision column",
// secondary actions under 更多. Live over SSE. See docs/design/2026-11-16-ui-v0.3.
import { $, $$, api, copyText, dur, esc, num, pad2, stamp, toast } from './util.js';
import { GRADE, md, inline } from './md.js';
import { S, MODES, ROLE_ZH, STATUS_ZH, FAMILY, WORKSHOP_STEPS, saveComposer, chip } from './state.js';
import { voiceButton, bindVoice } from './voice.js';
import { announce } from './attention.js';

const view = () => $('#view');
const live = (r) => r && (r.status === 'running' || r.status === 'awaiting_approval');
const PARALLEL_ROLES = ['planner', 'critic', 'panelist', 'proponent', 'opponent', 'judge'];
let headObserver = null;

document.addEventListener('click', (e) => {
  $$('.more-menu[open]').forEach((m) => { if (!m.contains(e.target) || e.target.closest('.menu')) m.open = false; });
});
document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape') return;
  const m = document.querySelector('.more-menu[open]');
  if (m) { m.open = false; m.querySelector('summary')?.focus(); }
});

export function closeStream() {
  if (S.es) { S.es.close(); S.es = null; }
  if (headObserver) { headObserver.disconnect(); headObserver = null; watchedTitle = null; }
}

export async function renderSession(id) {
  S.rec = null;
  S.expanded = new Set();
  S.diff = null;
  S.planEdit = null;
  S.phaseOpen = new Map(); // round → open, only where the user overrode the default
  S.gateUi = { reviseOpen: false, reviseText: '', verify: null };
  S.planTab = 'preview';
  view().innerHTML = `<div id="sHead" class="session-head"><a class="back" href="#/">← 議題</a><h1 id="sTitle" tabindex="-1">載入中…</h1></div>
    <div id="sSticky" class="session-sticky" hidden aria-hidden="true"></div>
    <div id="sStepper"></div>
    <div id="sGate"></div>
    <div id="sVerdict"></div>
    <div id="sWorkspace"></div>
    <div id="sPlanBox"></div>
    <div class="session-body">
      <aside class="rail" id="sRail" aria-label="議題概況"></aside>
      <div class="timeline-wrap">
        <div class="timeline-tools" id="sTools"></div>
        <div class="timeline" id="sTimeline"></div>
      </div>
    </div>
    <form class="interject" id="sInterject" hidden>
      <label class="field" style="flex:1"><span>插話（下一則發言起生效）</span><textarea name="text" id="interjectText" placeholder="補充條件、指出方向、要求聚焦……"></textarea></label>
      <div class="toolbar">${voiceButton('interjectText')}<button class="btn" type="submit">送出</button></div>
    </form>`;
  bindVoice(view());
  $('#sInterject').addEventListener('submit', async (e) => {
    e.preventDefault();
    const ta = e.target.text;
    if (!ta.value.trim()) return;
    try { await api(`/api/sessions/${encodeURIComponent(id)}/interject`, { method: 'POST', body: { text: ta.value } }); ta.value = ''; toast('已送出，下一則發言會看到'); }
    catch (err) { toast(err.message, true); }
  });
  openStream(id);
}

function openStream(id) {
  closeStream();
  const es = new EventSource(`/api/sessions/${encodeURIComponent(id)}/events`);
  S.es = es;
  es.onmessage = (m) => { let ev; try { ev = JSON.parse(m.data); } catch { return; } applyEvent(ev, id); };
  es.onerror = async () => {
    if (S.es !== es) return;
    if (S.rec && !live(S.rec)) { closeStream(); return; }
    try { const r = await api(`/api/sessions/${encodeURIComponent(id)}`); if (S.route.includes(id)) { S.rec = r; paintSession(); } }
    catch (e) { if (!S.rec) $('#sHead').innerHTML = `<a class="back" href="#/">← 議題</a><p class="lede">${esc(e.message)}</p>`; closeStream(); }
  };
}

// ------------------------------------------------------------------ scheduling

let paintQueued = false;
const dirtyTurns = new Set();
// requestAnimationFrame does not fire while the page is hidden (background tab, minimized
// desktop window); a timer fallback guarantees the view still catches up (lessons/0005).
function nextFrame(fn) {
  let done = false;
  const run = () => { if (!done) { done = true; fn(); } };
  requestAnimationFrame(run);
  setTimeout(run, 250);
}
function schedulePaint() {
  if (paintQueued) return;
  paintQueued = true;
  nextFrame(() => { paintQueued = false; dirtyTurns.clear(); paintSession(); });
}
function scheduleTurn(idx) {
  dirtyTurns.add(idx);
  if (paintQueued) return;
  paintQueued = true;
  nextFrame(() => {
    paintQueued = false;
    const stick = nearBottom();
    for (const i of dirtyTurns) {
      const el = document.getElementById(`turn-${i}`);
      const t = S.rec?.turns[i];
      if (el && t) {
        el.outerHTML = turnHtml(t);
        const mini = document.getElementById(`mini-${i}`);
        if (mini) mini.outerHTML = miniHtml(t);
      } else { dirtyTurns.clear(); paintSession(); return; }
    }
    dirtyTurns.clear();
    if (stick) window.scrollTo({ top: document.body.scrollHeight });
  });
}
const nearBottom = () => window.innerHeight + window.scrollY >= document.body.scrollHeight - 160;

export function applyEvent(ev, id) {
  // Ring buffer of recent event types — `window.__agora.evlog` when debugging the live view.
  (S.evlog ||= []).push(`${ev.type}${ev.status ? ':' + ev.status : ''}${ev.phase ? ':' + ev.phase : ''}`);
  if (S.evlog.length > 300) S.evlog.splice(0, 100);
  if (ev.type === 'quota') return; // handled by the global stream
  if (ev.type === 'snapshot') { S.rec = ev.session; paintSession(); return; }
  const r = S.rec;
  if (!r || (ev.session_id && ev.session_id !== r.id)) return;
  switch (ev.type) {
    case 'turn_started':
    case 'turn_ended':
      r.turns[ev.turn.index] = ev.turn;
      schedulePaint();
      break;
    case 'delta': {
      const t = r.turns[ev.index];
      if (!t || ev.seq <= t.delta_seq) return;
      t.text += ev.text;
      t.delta_seq = ev.seq;
      scheduleTurn(ev.index);
      break;
    }
    case 'tool': {
      const t = r.turns[ev.index];
      if (t) { t.tools.push(ev.note); scheduleTurn(ev.index); }
      break;
    }
    case 'note': r.notes.push(ev.note); schedulePaint(); break;
    case 'interjection': r.interjections.push(ev.interjection); schedulePaint(); break;
    case 'verify': r.verify_runs.push(ev.run); schedulePaint(); break;
    case 'verdict': r.verdict = ev.verdict; schedulePaint(); break;
    case 'phase':
      r.phase = ev.phase;
      if (r.status === 'running' && ev.phase) announce(`進入「${ev.phase}」階段`, `${r.id}:phase:${ev.phase}:${r.turns.length}`);
      schedulePaint();
      break;
    case 'plan': r.plan = ev.plan; if (!r.plan.approved) { S.planEdit = null; S.planTab = 'preview'; } schedulePaint(); break;
    case 'workspace': r.workspace = ev.workspace; S.diff = null; schedulePaint(); break;
    case 'status':
      r.status = ev.status; r.error = ev.error; r.updated_at = Date.now();
      if (!live(r)) r.phase = null;
      announce(ev.status === 'awaiting_approval' ? `方案已整理好，等你核准：${r.title}` : `議題${STATUS_ZH[ev.status] || ev.status}：${r.title}`, `${r.id}:${ev.status}`);
      schedulePaint();
      if (!live(r)) setTimeout(closeStream, 300);
      break;
    case 'lagged':
      api(`/api/sessions/${encodeURIComponent(id)}`).then((x) => { S.rec = x; paintSession(); }).catch(() => {});
      break;
    default: break;
  }
}

// ------------------------------------------------------------------ pieces

/** First real sentence of a turn: skips headings, code fences, tables and the mock prefix. */
function headline(text, max = 90) {
  const lines = String(text || '').split(/\r?\n/).map((l) => l.trim());
  const clean = (l) => l.replace(/^#+\s*/, '').replace(/^(\d+[.)]|[-*+>])\s+/, '')
    .replace(/^[（(]模擬[^)）]*[)）]\s*/, '').replace(/\*\*|`/g, '').trim();
  const usable = (l) => l && !l.startsWith('```') && !l.startsWith('|') && clean(l);
  const line = clean(lines.find((l) => usable(l) && !l.startsWith('#')) || lines.find(usable) || '');
  return line.length > max ? line.slice(0, max - 1) + '…' : line;
}

function turnHtml(t) {
  const running = !t.ended_at;
  const long = !running && t.text.length > 2400;
  const collapsed = long && !S.expanded.has(t.index);
  const tools = t.tools.length
    ? `<div class="tools">${t.tools.slice(-6).map((n) => `<span title="${esc(n.name)} ${esc(n.detail)}">${esc(n.name)}${n.detail ? ' · ' + esc(n.detail) : ''}</span>`).join('')}${t.tools.length > 6 ? `<span>＋${t.tools.length - 6}</span>` : ''}</div>`
    : '';
  const state = running ? '<span class="dot live"></span>撰寫中' : t.ok ? esc(dur((t.ended_at || 0) - t.started_at)) : '<span class="dot bad"></span>失敗';
  const u = t.usage;
  const foot = [
    u ? `輸入 ${num(u.input_tokens)}${u.cached_input_tokens ? `（快取 ${num(u.cached_input_tokens)}）` : ''} · 輸出 ${num(u.output_tokens)} tokens` : '',
    t.context_compacted ? '上下文已記憶壓縮' : '',
    t.rerouted_from ? `額度保護：由 ${t.rerouted_from} 改派` : '',
    t.seeded ? '承接自上一個議題' : '',
  ].filter(Boolean);
  const wide = ['synthesizer', 'builder', 'reviewer', 'solo'].includes(t.role) || (t.role === 'judge' && S.rec.turns.filter((x) => x.role === 'judge').length === 1);
  const canFork = !running && t.ok && !live(S.rec);
  return `<article class="turn fam-${t.family}${collapsed ? ' collapsed' : ''}${wide ? ' wide' : ''}${t.seeded ? ' seeded' : ''}" id="turn-${t.index}">
    <header><span class="who">${esc(FAMILY[t.family] || t.family)}</span><span class="role">${esc(ROLE_ZH[t.role] || t.role)}</span>${t.model ? `<span class="model">${esc(t.model)}</span>` : ''}<span class="state">${state}</span>
      ${canFork ? `<button class="fork" type="button" data-fork="${t.index}" title="從這則之後分岔出新議題">分岔 ↗</button>` : ''}</header>
    ${tools}
    <div class="body md${running ? ' cursor' : ''}">${t.text ? md(t.text) : running ? '<p class="muted">等待模型回應…</p>' : ''}</div>
    ${t.error ? `<div class="err">${esc(t.error)}</div>` : ''}
    ${long ? `<button class="expand" type="button" data-expand="${t.index}">${collapsed ? '展開全文 ↓' : '收合 ↑'}</button>` : ''}
    ${foot.length ? `<footer>${foot.map(esc).join('<span aria-hidden="true">·</span>')}</footer>` : ''}
  </article>`;
}

/** One line per turn in a folded round: who, role, first sentence, time, tokens. */
/** First sentence of the first real line (。！？, or . ! ? followed by a space). */
function firstSentence(text, max = 80) {
  const line = headline(text, 400);
  const m = line.match(/^(.+?(?:[。！？]|[.!?](?=\s)))/);
  const out = (m ? m[1] : line).trim();
  return out.length > max ? out.slice(0, max - 1) + '…' : out;
}

/** One line per turn in a folded round: who · role, first sentence, model · time · tokens. */
function miniHtml(t) {
  const running = !t.ended_at;
  const u = t.usage;
  const text = running ? (t.text ? firstSentence(t.text) : '等待模型回應…') : !t.ok ? `失敗：${t.error || '沒有回應'}` : firstSentence(t.text) || '（沒有文字內容）';
  const meta = [t.model, running ? '撰寫中' : dur((t.ended_at || 0) - t.started_at), !running && u ? `${num(u.input_tokens + u.output_tokens)} tokens` : ''].filter(Boolean).join(' · ');
  return `<li class="mini fam-${t.family}${!running && !t.ok ? ' failed' : ''}" id="mini-${t.index}" data-turn="${t.index}"><b>${esc(FAMILY[t.family] || t.family)} · ${esc(ROLE_ZH[t.role] || t.role)}</b><span class="mini-text">${esc(text)}</span><span class="mini-meta">${esc(meta)}</span></li>`;
}

function verifyHtml(v) {
  return `<div class="verify ${v.ok ? 'ok' : 'fail'} wide"><header><span class="dot ${v.ok ? 'ok' : 'bad'}"></span><b>${v.ok ? '驗證通過' : v.timed_out ? '驗證逾時' : '驗證失敗'}</b><code>$ ${esc(v.command)}</code><span class="muted" style="margin-left:auto">exit ${v.exit_code ?? '—'} · ${esc(dur(v.duration_ms))}</span></header>${v.output_tail ? `<details><summary class="muted" style="padding:0 .9rem .6rem;cursor:pointer;font-size:.8rem">輸出尾段</summary><pre>${esc(v.output_tail)}</pre></details>` : ''}</div>`;
}

function roundTitle(r, round, items) {
  const roles = items.filter((x) => x.kind === 'turn').map((x) => x.t.role);
  const seeded = items.every((x) => x.kind === 'turn' && x.t.seeded);
  const pre = seeded ? '承接 · ' : '';
  if (roles.includes('planner')) return `${pre}獨立提案`;
  if (roles.includes('critic')) return roles.length === 1 ? `${pre}紅隊審視` : `${pre}互評`;
  if (roles.includes('synthesizer')) return r.spec.mode === 'workshop' ? `${pre}整合方案` : `${pre}總結`;
  if (roles.includes('judge')) return `${pre}裁決`;
  if (roles.includes('builder') || roles.includes('reviewer')) return `${pre}實作與審查`;
  if (r.spec.mode === 'solo') return `${pre}回答`;
  if (r.spec.mode === 'debate') return `${pre}${round === 1 ? '開場陳述' : '反駁'}`;
  return `${pre}${round === 1 ? '獨立發言' : '回應'}`;
}

function stepperHtml(r) {
  if (r.spec.mode !== 'workshop') return '';
  let steps = WORKSHOP_STEPS.slice();
  if (r.spec.plan_only) steps = steps.slice(0, 4);
  const phase = r.phase || '';
  const map = { 紅隊審視: '整合', 修訂方案: '整合', 已收到決定: '等待核准', 修訂實作: '實作' };
  const cur = steps.indexOf(map[phase] || phase);
  const finished = !live(r);
  const reached = finished ? (r.turns.some((t) => t.role === 'builder') ? steps.length : 4) : cur;
  return `<ol class="stepper" aria-label="方案工作坊進度">${steps.map((s, i) => {
    const cls = finished ? (i < reached ? 'done' : 'skip') : i < cur ? 'done' : i === cur ? 'now' : '';
    return `<li class="${cls}"${cls === 'now' ? ' aria-current="step"' : ''}><span>${pad2(i + 1)}</span>${esc(s)}</li>`;
  }).join('')}</ol>`;
}

function suggestedVerify(planText) {
  const m = String(planText || '').match(/建議的驗證指令[^\n]*\n+([\s\S]*?)(\n#|$)/);
  if (!m) return '';
  const code = m[1].match(/`([^`\n]+)`/);
  return code ? code[1].trim() : '';
}

/** Approval gate: one document scroll, sticky decision column (desktop), ARIA tabs. */
function gateHtml(r) {
  const p = r.plan;
  if (!p || r.status !== 'awaiting_approval') return '';
  const editing = S.planTab === 'edit'; // which tab is shown; S.planEdit holds the user's text
  const verify = p.verify_command || r.spec.verify_command || '';
  const suggested = suggestedVerify(p.text);
  return `<section class="plan-gate" aria-labelledby="gateTitle">
    <div class="top"><span class="eyebrow">Approval · 等你核准</span><span class="status-pill"><span class="dot live"></span>第 ${p.revision + 1} 版方案${p.feedback.length ? ` · 已修訂 ${p.feedback.length} 次` : ''}</span></div>
    <h2 id="gateTitle">兩個模型討論出來的方案在這裡。看過、需要的話改一改，再決定要不要實作。</h2>
    <a class="jump-decide" href="#planActions" data-jump-decide="1">跳到核准決定 ↓</a>
    <div class="plan-layout">
      <div class="plan-document">
        <div class="seg" role="tablist" aria-label="方案檢視方式">
          <button type="button" role="tab" id="tab-preview" aria-controls="panel-plan" aria-selected="${!editing}" tabindex="${editing ? -1 : 0}" data-plan-tab="preview" class="${editing ? '' : 'on'}">預覽</button>
          <button type="button" role="tab" id="tab-edit" aria-controls="panel-plan" aria-selected="${editing}" tabindex="${editing ? 0 : -1}" data-plan-tab="edit" class="${editing ? 'on' : ''}">編輯</button>
        </div>
        <div role="tabpanel" id="panel-plan" aria-labelledby="${editing ? 'tab-edit' : 'tab-preview'}">
          ${editing ? `<label class="sr" for="planText">編輯方案</label><textarea id="planText" class="plan-text">${esc(S.planEdit)}</textarea>` : `<div class="md plan-preview">${md(S.planEdit ?? p.text)}</div>`}
        </div>
      </div>
      <aside class="plan-actions" id="planActions" aria-label="核准決定">
        ${r.spec.plan_only ? '<p class="hint">這個議題只產出方案；核准後就結束，不會實作。</p>' : `<label class="field"><span>驗證指令</span><input type="text" id="planVerify" value="${esc(S.gateUi.verify ?? verify)}" placeholder="${esc(suggested ? `方案建議：${suggested}` : '例如 cargo test')}"><small>強烈建議：有它才可能得到「已確認」</small></label>`}
        <button class="btn primary block" type="button" id="approveBtn">${r.spec.plan_only ? '核准方案' : '核准並開始實作 ↗'}</button>
        <button class="btn block" type="button" id="reviseToggle" aria-expanded="${S.gateUi.reviseOpen}" aria-controls="reviseBox"${p.revision >= 2 ? ' disabled title="已達修訂上限（2 次）"' : ''}>要求修訂${p.revision >= 2 ? '（已達上限）' : ''}</button>
        <div id="reviseBox" class="revise-box"${S.gateUi.reviseOpen ? '' : ' hidden'}>
          <label class="field"><span>要怎麼改？</span><textarea id="reviseText" placeholder="例如：步驟 3 太大，拆成兩步；不要動到 API 介面">${esc(S.gateUi.reviseText)}</textarea></label>
          <div class="toolbar">${voiceButton('reviseText')}<button class="btn small" type="button" id="reviseBtn">送出修訂要求</button></div>
        </div>
        <button class="btn quiet block" type="button" id="rejectBtn">只保留方案，不實作</button>
        <p class="hint">${S.planEdit != null && S.planEdit !== p.text ? '你改過方案：核准時會用你改過的版本。' : '可以切到「編輯」直接改方案文字。'}</p>
      </aside>
    </div>
  </section>`;
}

function planBoxHtml(r) {
  const p = r.plan;
  if (!p || r.status === 'awaiting_approval') return '';
  return `<details class="plan-box"${!r.verdict && live(r) ? ' open' : ''}><summary><span class="eyebrow">Plan · 方案</span> ${p.approved ? '已核准' : '未核准'}（第 ${p.revision + 1} 版${p.edited ? '，經你修改' : ''}）</summary><div class="md">${md(p.text)}</div>
    ${p.feedback.length ? `<p class="muted">你提出的修訂要求：${p.feedback.map(esc).join('；')}</p>` : ''}</details>`;
}

function workspaceHtml(r) {
  const ws = r.workspace;
  if (!ws) return '';
  const wt = ws.kind === 'worktree';
  const stateZh = { active: wt ? '隔離中' : '已直接修改', applied: '已套用到專案', discarded: '已丟棄' }[ws.state];
  const canAct = wt && ws.state === 'active' && !live(r);
  const d = S.diff;
  return `<section class="workspace" aria-label="工作區">
    <div class="top"><span class="eyebrow">Workspace · ${wt ? '隔離工作區' : '專案資料夾'}</span><span class="chip">${esc(stateZh)}</span>${ws.branch ? `<code class="muted">${esc(ws.branch)}</code>` : ''}</div>
    <p class="muted" style="margin:0">${wt ? `模型在 <code>${esc(ws.path)}</code> 工作，你的專案資料夾在套用前不會被改動。` : '這個議題直接修改專案資料夾（不是 git repo 或已關閉隔離）。'}${ws.message ? ' ' + esc(ws.message) : ''}</p>
    <div class="toolbar">
      ${ws.state === 'active' ? `<button class="btn small" type="button" id="diffBtn"${S.diffLoading ? ' disabled aria-busy="true"' : ''}>${S.diffLoading ? '載入 diff 中…' : d ? '重新整理 diff' : '查看 diff'}</button>` : ''}
      ${canAct ? '<button class="btn small primary" type="button" id="applyBtn">套用到專案（不 commit）</button><button class="btn small danger" type="button" id="discardBtn">丟棄</button>' : ''}
      ${wt && ws.state === 'active' && live(r) ? '<span class="muted">議題結束後才能套用或丟棄</span>' : ''}
    </div>
    ${d ? diffHtml(d) : ''}
  </section>`;
}

function diffHtml(d) {
  if (d.error) return `<div class="err">${esc(d.error)}</div>`;
  if (!d.files.length) return '<p class="muted">沒有任何變更。</p>';
  const files = d.files.map((f) => `<li><code>${esc(f.path)}</code><span class="add">+${f.added ?? '?'}</span><span class="del">−${f.removed ?? '?'}</span></li>`).join('');
  const lines = d.patch.split('\n').slice(0, 4000).map((l) => {
    const cls = l.startsWith('diff --git') ? 'file' : l.startsWith('@@') ? 'hunk' : l.startsWith('+') && !l.startsWith('+++') ? 'add' : l.startsWith('-') && !l.startsWith('---') ? 'del' : '';
    return `<span class="${cls}">${esc(l)}</span>`;
  }).join('\n');
  return `<div class="diff"><p><b>${d.files.length}</b> 個檔案 · <span class="add">+${d.added}</span> <span class="del">−${d.removed}</span>${d.truncated ? '（內容過長，只顯示前段）' : ''}</p>
    <ul class="diff-files">${files}</ul><details open><summary class="muted">逐行差異</summary><pre class="patch">${lines}</pre></details></div>`;
}

function verdictHtml(v) {
  if (!v) return '';
  return `<section class="verdict" aria-label="結論">
      <div class="top"><span class="eyebrow">Verdict · 結論</span><span class="grade ${GRADE[v.grade].cls}" title="${esc(GRADE[v.grade].tip)}">證據等級 · ${GRADE[v.grade].zh}</span></div>
      <h2>${esc(v.headline)}</h2>
      ${v.summary ? `<div class="md">${md(v.summary)}</div>` : ''}
      ${v.open_issues.length ? `<div><p class="eyebrow" style="margin-bottom:.4rem">${{ synthesized: '待驗證事項', judged: '待驗證事項', split: '待驗證事項', planned: '驗收標準' }[v.outcome] || '未解決的問題'}</p><ul>${v.open_issues.map((x) => `<li>${inline(x)}</li>`).join('')}</ul></div>` : ''}
      <p class="explain">「${GRADE[v.grade].zh}」：${esc(GRADE[v.grade].tip)}。${v.grade === 'confirmed' ? '' : '要升到「已確認」，需要實際執行驗證指令並通過。'}</p>
    </section>`;
}

/** Rounds of the transcript, each a <details> with one-line summaries per turn. */
function groupedRounds(r) {
  const items = [
    ...r.turns.map((t) => ({ kind: 'turn', round: t.round, at: t.started_at, idx: t.index, t })),
    ...r.verify_runs.map((vr, k) => ({ kind: 'verify', round: vr.round, at: vr.started_at, idx: 1000 + k, v: vr })),
  ];
  const rounds = [...new Set(items.map((x) => x.round))].sort((a, b) => a - b);
  return rounds.map((rd) => ({ round: rd, items: items.filter((x) => x.round === rd).sort((a, b) => a.at - b.at || a.idx - b.idx) }));
}

const latestRound = (r) => r.turns.reduce((m, t) => Math.max(m, t.round), 0);
/** Running: only the newest round is open. Awaiting approval / finished: all folded. */
const autoOpen = (r, round) => r.status === 'running' && round === latestRound(r);
function defaultOpen(r, round) {
  return S.phaseOpen.has(round) ? S.phaseOpen.get(round) : autoOpen(r, round);
}

function timelineHtml(r) {
  const groups = groupedRounds(r);
  if (!groups.length) return '<p class="lede">等待第一則發言…</p>';
  return groups.map(({ round, items }) => {
    const open = defaultOpen(r, round);
    const turns = items.filter((x) => x.kind === 'turn');
    const parallel = turns.filter((x) => PARALLEL_ROLES.includes(x.t.role)).length > 1;
    const running = turns.some((x) => !x.t.ended_at);
    const verifies = items.filter((x) => x.kind === 'verify');
    const count = `${turns.length} 則${verifies.length ? ` · 驗證 ${verifies.filter((x) => x.v.ok).length}/${verifies.length}` : ''}`;
    return `<section class="phase${open ? ' open' : ''}" id="phase-${round}" data-round="${round}">
      <h3 class="phase-head"><button type="button" class="phase-toggle" id="phase-${round}-t" aria-expanded="${open}" aria-controls="phase-${round}-body" data-phase="${round}"><span class="phase-title">${pad2(round)} · ${esc(roundTitle(r, round, items))}</span>${running ? '<span class="dot live" aria-hidden="true"></span><span class="sr">進行中</span>' : ''}<span class="sr">，</span><span class="phase-count">${count}</span></button></h3>
      <ul class="minis" aria-label="本輪摘要"${open ? ' hidden' : ''}>${turns.map((x) => miniHtml(x.t)).join('')}</ul>
      <div class="round-items${parallel ? ' pair' : ''}" id="phase-${round}-body"${open ? '' : ' hidden'}>${items.map((x) => (x.kind === 'turn' ? turnHtml(x.t) : verifyHtml(x.v))).join('')}</div>
    </section>`;
  }).join('');
}

function railHtml(r, isLive) {
  const own = r.turns.filter((t) => !t.seeded);
  const usage = own.reduce((a, t) => { if (t.usage) { a.i += t.usage.input_tokens; a.o += t.usage.output_tokens; } return a; }, { i: 0, o: 0 });
  const budgetNote = r.notes.find((n) => /最多 \d+ 次/.test(n.text));
  const budget = budgetNote ? budgetNote.text.match(/最多 (\d+) 次/)[1] : '—';
  const log = [
    ...r.notes.map((n) => ({ at: n.at, cls: n.level, text: n.text })),
    ...r.interjections.map((ij) => ({ at: ij.at, cls: 'user', text: `你插話：${ij.text}${ij.delivered ? '' : '（等待送達）'}` })),
  ].sort((a, b) => a.at - b.at);
  const groups = groupedRounds(r);
  const guards = log.filter((l) => l.cls === 'guard').length;
  const logOpen = S.logOpen ?? window.innerWidth > 980;
  return `<nav class="phase-index" aria-label="階段"><h3>階段</h3><ol>${groups.map(({ round, items }) => `<li><a href="#phase-${round}" data-jump="${round}">${pad2(round)} · ${esc(roundTitle(r, round, items))}</a></li>`).join('') || '<li class="muted">—</li>'}</ol></nav>
    <div><h3>概況</h3>
      <div class="stat"><span>發言</span><b>${own.length} / ${esc(budget)}</b></div>
      <div class="stat"><span>Tokens（輸入／輸出）</span><b>${num(usage.i)} / ${num(usage.o)}</b></div>
      <div class="stat"><span>經過時間</span><b>${esc(dur((isLive ? Date.now() : r.updated_at) - r.created_at))}</b></div>
      ${r.verify_runs.length ? `<div class="stat"><span>驗證</span><b>${r.verify_runs.filter((x) => x.ok).length} / ${r.verify_runs.length} 通過</b></div>` : ''}
    </div>
    <details class="log-box"${logOpen ? ' open' : ''}><summary><h3>過程紀錄 <span class="muted">${log.length}${guards ? ` · 防護 ${guards}` : ''}</span></h3></summary><div class="log">${log.map((l) => `<div class="item ${esc(l.cls)}"><time>${esc(stamp(l.at).slice(11))}</time>${esc(l.text)}</div>`).join('') || '<div class="item">—</div>'}</div></details>`;
}

function headHtml(r, isLive) {
  const proj = S.projects.find((p) => p.id === r.spec.project_id);
  const m = MODES[r.spec.mode];
  const done = !isLive;
  const status = r.status === 'running' && r.phase ? r.phase : STATUS_ZH[r.status];
  return `<a class="back" href="#/">← 議題</a>
    <p class="eyebrow">${m.en} · ${m.zh}${proj ? ' · ' + esc(proj.name) : ''} · ${esc(stamp(r.created_at))}${r.spec.blind ? ' · 盲審' : ''}${r.spec.group_id ? ` · <a href="#/x/${esc(r.spec.group_id)}">同題比較 ↗</a>` : ''}</p>
    <h1 id="sTitle" tabindex="-1">${esc(r.title)}</h1>
    <div class="toolbar">
      <span class="status-pill" id="sStatus" tabindex="-1"><span class="dot ${isLive ? 'live' : r.status === 'done' ? 'ok' : 'bad'}"></span>${esc(status)}</span>
      <div class="chips">${r.spec.participants.map(chip).join('')}</div><span class="spacer"></span>
      ${isLive ? '<button class="btn danger small" type="button" id="cancelBtn">停止</button>' : ''}
      ${done && r.verdict ? '<button class="btn small" type="button" id="rememberBtn">存入專案記憶</button>' : ''}
      <details class="more-menu"><summary class="btn small quiet" id="moreBtn">更多 ▾</summary><div class="menu">
        ${done && r.verdict ? '<button type="button" id="copyBtn">複製結論</button>' : ''}
        <a href="/api/sessions/${esc(r.id)}/export.md" download>下載 Markdown</a>
        ${done && S.whoami?.local ? `<button type="button" data-export="adr">寫成 ADR</button>${r.status !== 'done' || (r.verdict && r.verdict.grade !== 'confirmed') ? '<button type="button" data-export="lesson">產生 lesson 草稿</button>' : ''}` : ''}
        <button type="button" id="reuseBtn">以此為範本</button>
      </div></details>
    </div>
    ${r.error ? `<div class="banner bad"><b>議題中止</b><span>${esc(r.error)}</span></div>` : ''}
    <details class="brief"><summary>議題內容${r.spec.verify_command ? ' · 驗證指令 ' + esc(r.spec.verify_command) : ''}${r.spec.access === 'write' ? ' · 可修改檔案' : ''}</summary><pre>${esc(r.spec.prompt)}</pre></details>`;
}

function stickyHtml(r, isLive) {
  const status = r.status === 'running' && r.phase ? r.phase : STATUS_ZH[r.status];
  return `<span class="dot ${isLive ? 'live' : r.status === 'done' ? 'ok' : 'bad'}"></span><b>${esc(r.title)}</b><span class="muted">${esc(status)}</span>
    ${r.status === 'awaiting_approval' ? '<a class="btn small primary" href="#sGate" data-jump-gate="1">前往核准 ↑</a>' : ''}`;
}

// ------------------------------------------------------------------ paint

function focusKey(a) {
  if (!a || a === document.body) return null;
  if (a.id) return `#${CSS.escape(a.id)}`;
  if (a.tagName === 'SUMMARY' && a.parentElement?.id) return `#${CSS.escape(a.parentElement.id)} > summary`;
  const d = Object.entries(a.dataset)[0];
  return d ? `[data-${d[0].replace(/[A-Z]/g, (c) => '-' + c.toLowerCase())}="${CSS.escape(d[1])}"]` : null;
}

/** Replace a region only when its markup changed, keeping focus and an open 更多 menu. */
function swap(el, html) {
  if (!el || el._html === html) return;
  const a = document.activeElement;
  const key = el.contains(a) ? focusKey(a) : null;
  const menuOpen = el.querySelector('.more-menu[open]') != null;
  el.innerHTML = html;
  el._html = html;
  if (menuOpen) { const m = el.querySelector('.more-menu'); if (m) m.open = true; }
  if (key) el.querySelector(key)?.focus({ preventScroll: true });
}

export function paintSession() {
  const r = S.rec;
  if (!r) return;
  const stick = nearBottom() && r.status === 'running';
  const isLive = live(r);
  if (r.status !== 'awaiting_approval') { S.planEdit = null; S.planTab = 'preview'; S.gateUi = { reviseOpen: false, reviseText: '', verify: null }; }

  swap($('#sHead'), headHtml(r, isLive));
  swap($('#sSticky'), stickyHtml(r, isLive));
  swap($('#sStepper'), stepperHtml(r));
  // Never rewrite the gate under the user's cursor while it is still waiting for them.
  const gate = $('#sGate');
  if (!(r.status === 'awaiting_approval' && gate._html && gate.contains(document.activeElement))) swap(gate, gateHtml(r));
  swap($('#sVerdict'), verdictHtml(r.verdict));
  swap($('#sWorkspace'), workspaceHtml(r));
  swap($('#sPlanBox'), planBoxHtml(r));
  swap($('#sTools'), r.turns.length > 1 ? `<span class="eyebrow">Discussion · 討論過程</span><span class="spacer"></span><button class="btn small quiet" type="button" data-fold="open">展開所有輪次</button><button class="btn small quiet" type="button" data-fold="close">收合所有輪次</button>` : '');
  swap($('#sTimeline'), timelineHtml(r));
  swap($('#sRail'), railHtml(r, isLive));

  const ij = $('#sInterject');
  if (ij) ij.hidden = r.status !== 'running';
  bindSession();
  watchHead();
  // After approve/revise: move focus once the server's new status is on screen, not before.
  if (S.pendingFocus && r.status !== 'awaiting_approval') { S.pendingFocus = false; $('#sStatus')?.focus(); }
  if (stick) window.scrollTo({ top: document.body.scrollHeight });
}

/** Show the slim sticky bar once the title scrolls out of view. */
let watchedTitle = null;
/**
 * Show the slim fixed bar once the title scrolls under the top bar. The head is re-rendered on
 * updates, so re-observe whenever the <h1> element itself changes (a detached node reports
 * "not intersecting" forever).
 */
function watchHead() {
  if (!('IntersectionObserver' in window)) return;
  const h1 = $('#sHead h1');
  const bar = $('#sSticky');
  if (!h1 || !bar || h1 === watchedTitle) return;
  if (headObserver) headObserver.disconnect();
  watchedTitle = h1;
  const top = $('.topbar')?.offsetHeight || 0;
  headObserver = new IntersectionObserver(([e]) => {
    const show = !e.isIntersecting && e.boundingClientRect.top < top;
    bar.hidden = !show;
    bar.setAttribute('aria-hidden', String(!show));
  }, { rootMargin: `-${top}px 0px 0px 0px` });
  headObserver.observe(h1);
}

async function loadDiff() {
  S.diffLoading = true;
  swap($('#sWorkspace'), workspaceHtml(S.rec));
  try { S.diff = await api(`/api/sessions/${encodeURIComponent(S.rec.id)}/diff`); }
  catch (e) { S.diff = { error: e.message, files: [] }; }
  S.diffLoading = false;
  swap($('#sWorkspace'), workspaceHtml(S.rec));
  bindSession();
}

const motion = () => (matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth');

/** Open/close one round in place; remember it only when it differs from the automatic rule. */
function setPhaseOpen(round, open) {
  round = Number(round);
  const sec = document.getElementById(`phase-${round}`);
  if (!sec) return;
  sec.classList.toggle('open', open);
  sec.querySelector('.phase-toggle')?.setAttribute('aria-expanded', String(open));
  const minis = sec.querySelector('.minis');
  if (minis) minis.hidden = open;
  const body = sec.querySelector('.round-items');
  if (body) body.hidden = !open;
  if (open === autoOpen(S.rec, round)) S.phaseOpen.delete(round); else S.phaseOpen.set(round, open);
  $('#sTimeline')._html = null; // the DOM no longer matches the cached markup
}

function openPhase(round) {
  const el = document.getElementById(`phase-${round}`);
  if (!el) return;
  setPhaseOpen(round, true);
  el.scrollIntoView({ behavior: motion(), block: 'start' });
  el.querySelector('.phase-toggle')?.focus({ preventScroll: true });
}

// Voice buttons are delegated once on the view root (renderSession); never rebind here.
function repaintGate(r) {
  const gate = $('#sGate');
  gate.innerHTML = gate._html = gateHtml(r);
  bindSession();
}

function bindSession() {
  const r = S.rec;
  const id = r.id;
  const on = (sel, fn) => { const el = $(sel); if (el) el.onclick = fn; };
  on('#cancelBtn', async () => { if (!confirm('停止這個議題？進行中的 CLI 會被終止。')) return; try { await api(`/api/sessions/${encodeURIComponent(id)}/cancel`, { method: 'POST', body: {} }); } catch (e) { toast(e.message, true); } });
  on('#rememberBtn', async () => {
    try {
      const p = await api(`/api/sessions/${encodeURIComponent(id)}/remember`, { method: 'POST', body: {} });
      S.projects = S.projects.map((x) => (x.id === p.id ? p : x));
      toast('已加入專案記憶，之後的議題都會帶入');
    } catch (e) { toast(e.message, true); }
  });
  on('#copyBtn', async () => {
    const v = r.verdict;
    const text = `# ${r.title}\n\n**${v.headline}**（證據等級：${GRADE[v.grade].zh}）\n\n${v.summary}\n${v.open_issues.length ? '\n' + v.open_issues.map((x) => `- ${x}`).join('\n') + '\n' : ''}`;
    toast((await copyText(text)) ? '已複製' : '無法複製');
  });
  on('#reuseBtn', () => {
    const s = r.spec;
    S.c.mode = s.mode;
    S.c.cast[s.mode] = s.participants.map((p) => ({ provider: p.provider, model: p.model || '' }));
    if (['roundtable', 'debate', 'workshop'].includes(s.mode)) S.c.judge[s.mode] = s.judge ? s.judge.provider : '';
    if (s.rounds != null && s.mode !== 'solo') S.c.rounds[s.mode] = s.rounds;
    Object.assign(S.c, { access: s.access, verify: s.verify_command || '', prompt: s.prompt, blind: s.blind, redTeam: s.red_team, planOnly: s.plan_only, autoApprove: false, seed: null });
    S.projectId = s.project_id;
    saveComposer();
    location.hash = '#/';
  });
  $$('[data-export]', view()).forEach((b) => {
    b.onclick = async () => {
      const kind = b.dataset.export;
      if (!confirm(kind === 'adr' ? '在專案的 docs/adr/ 新增一份 ADR？' : '在專案的 docs/lessons/ 新增一份 lesson 草稿？')) return;
      try { const x = await api(`/api/sessions/${encodeURIComponent(id)}/export`, { method: 'POST', body: { kind } }); toast(`已寫入 ${x.path}`); }
      catch (e) { toast(e.message, true); }
    };
  });

  // Approval gate — tabs (click + arrow keys), approve, revise, reject.
  const tabs = $$('[data-plan-tab]', view());
  const switchTab = (which) => {
    const ta = $('#planText');
    if (which === 'edit') S.planEdit = S.planEdit ?? r.plan.text;
    else if (ta) S.planEdit = ta.value === r.plan.text ? null : ta.value;
    S.planTab = which;
    repaintGate(r);
    $(`#tab-${which}`)?.focus();
  };
  tabs.forEach((b) => {
    b.onclick = () => switchTab(b.dataset.planTab);
    b.onkeydown = (e) => {
      const to = { ArrowRight: 'other', ArrowLeft: 'other', Home: 'preview', End: 'edit' }[e.key];
      if (!to) return;
      e.preventDefault();
      switchTab(to === 'other' ? (b.dataset.planTab === 'edit' ? 'preview' : 'edit') : to);
    };
  });
  const planText = $('#planText');
  if (planText) {
    const grow = () => { planText.style.height = 'auto'; planText.style.height = `${planText.scrollHeight + 2}px`; };
    grow();
    planText.oninput = () => { S.planEdit = planText.value; grow(); };
  }
  const pv = $('#planVerify');
  if (pv) pv.oninput = () => { S.gateUi.verify = pv.value; };
  const rt = $('#reviseText');
  if (rt) rt.oninput = () => { S.gateUi.reviseText = rt.value; };
  on('#approveBtn', async () => {
    const ta = $('#planText');
    const plan = ta ? ta.value : S.planEdit;
    const verify = $('#planVerify')?.value ?? null;
    try {
      await api(`/api/sessions/${encodeURIComponent(id)}/decide`, { method: 'POST', body: { action: 'approve', plan: plan ?? null, verify_command: verify } });
      S.planEdit = null;
      S.pendingFocus = true;
      toast(r.spec.plan_only ? '方案已核准' : '已核准，開始在隔離工作區實作');
    } catch (e) { toast(e.message, true); }
  });
  on('#reviseToggle', (e) => {
    const b = $('#reviseBox');
    b.hidden = !b.hidden;
    S.gateUi.reviseOpen = !b.hidden;
    e.currentTarget.setAttribute('aria-expanded', String(!b.hidden));
    if (!b.hidden) $('#reviseText').focus();
  });
  on('#reviseBtn', async () => {
    const feedback = $('#reviseText').value.trim();
    if (!feedback) return;
    try {
      await api(`/api/sessions/${encodeURIComponent(id)}/decide`, { method: 'POST', body: { action: 'revise', feedback } });
      S.pendingFocus = true;
      toast('已送出，總結者正在修訂');
    } catch (e) { toast(e.message, true); }
  });
  on('#rejectBtn', async () => {
    if (!confirm('保留方案但不實作？')) return;
    try { await api(`/api/sessions/${encodeURIComponent(id)}/decide`, { method: 'POST', body: { action: 'reject' } }); }
    catch (e) { toast(e.message, true); }
  });

  on('[data-jump-decide]', (e) => {
    e.preventDefault(); // hash links would hit the router
    $('#planActions')?.scrollIntoView({ behavior: motion(), block: 'start' });
    $('#approveBtn')?.focus({ preventScroll: true });
  });

  // Workspace
  on('#diffBtn', loadDiff);
  on('#applyBtn', async () => {
    if (!confirm('把隔離工作區的變更套用到你的專案資料夾？（不會 commit，你可以先檢查再自己 commit）')) return;
    try { S.rec = await api(`/api/sessions/${encodeURIComponent(id)}/workspace/apply`, { method: 'POST', body: {} }); S.diff = null; paintSession(); toast('已套用到專案'); }
    catch (e) { toast(e.message, true); }
  });
  on('#discardBtn', async () => {
    if (!confirm('丟棄隔離工作區的所有變更？這個動作無法復原。')) return;
    try { S.rec = await api(`/api/sessions/${encodeURIComponent(id)}/workspace/discard`, { method: 'POST', body: {} }); S.diff = null; paintSession(); toast('已丟棄'); }
    catch (e) { toast(e.message, true); }
  });

  // Fold controls, phase index, user toggles
  const tools = $('#sTools');
  if (tools) tools.onclick = (e) => {
    const b = e.target.closest('[data-fold]');
    if (!b) return;
    const open = b.dataset.fold === 'open';
    $$('.phase', view()).forEach((sec) => setPhaseOpen(sec.dataset.round, open));
  };
  const rail = $('#sRail');
  if (rail) {
    rail.onclick = (e) => {
      const a = e.target.closest('[data-jump]');
      if (!a) return;
      e.preventDefault();
      openPhase(a.dataset.jump);
    };
    const lb = rail.querySelector('.log-box');
    if (lb) lb.ontoggle = () => { S.logOpen = lb.open; };
  }
  const sticky = $('#sSticky');
  if (sticky) sticky.onclick = (e) => {
    if (!e.target.closest('[data-jump-gate]')) return;
    e.preventDefault();
    // Narrow screens stack the decision column under the whole plan: land on the buttons.
    const narrow = matchMedia('(max-width: 980px)').matches;
    (narrow ? $('#planActions') : $('#sGate'))?.scrollIntoView({ behavior: motion(), block: 'start' });
    $('#approveBtn')?.focus({ preventScroll: true });
  };

  // Timeline: expand long turns, fork
  $('#sTimeline').onclick = (e) => {
    const tg = e.target.closest('[data-phase]');
    if (tg) { setPhaseOpen(tg.dataset.phase, tg.getAttribute('aria-expanded') !== 'true'); return; }
    const mi = e.target.closest('.mini');
    if (mi) {
      setPhaseOpen(mi.closest('.phase').dataset.round, true);
      document.getElementById(`turn-${mi.dataset.turn}`)?.scrollIntoView({ behavior: motion(), block: 'start' });
      return;
    }
    const ex = e.target.closest('[data-expand]');
    if (ex) {
      const i = Number(ex.dataset.expand);
      if (S.expanded.has(i)) S.expanded.delete(i); else S.expanded.add(i);
      const el = document.getElementById(`turn-${i}`);
      if (el) el.outerHTML = turnHtml(S.rec.turns[i]);
      return;
    }
    const fk = e.target.closest('[data-fork]');
    if (fk) {
      const upto = Number(fk.dataset.fork);
      S.c.seed = { session_id: r.id, upto, title: r.title };
      S.c.mode = r.spec.mode === 'solo' ? 'roundtable' : r.spec.mode;
      S.c.prompt = r.spec.prompt;
      S.projectId = r.spec.project_id;
      saveComposer();
      location.hash = '#/';
      toast(`分岔：會延續前 ${upto + 1} 則發言，請調整議題或模式後開始`);
    }
  };
}
