// Settings: phone pairing (+QR), daily digest, schedules, default models, guards, templates,
// CLI paths, MCP, misc.
import { $, $$, api, copyText, esc, rel, stamp, toast, until } from './util.js';
import { S, MODES, MODE_ORDER, providerById, modelOptions, providerOptions, pickProvider, loadProviders } from './state.js';

const view = () => $('#view');
const SOURCES = [['hn', 'Hacker News'], ['github', 'GitHub Trending'], ['lobsters', 'Lobsters'], ['reddit', 'Reddit'], ['devto', 'dev.to'], ['hf', 'Hugging Face Papers']];
const WEEK = [[1, '一'], [2, '二'], [3, '三'], [4, '四'], [5, '五'], [6, '六'], [7, '日']];

export function notifyButton() {
  if (!('Notification' in window)) return '<span class="muted">這個瀏覽器不支援通知</span>';
  if (!window.isSecureContext) return '<span class="muted" style="font-size:.85rem">通知需要 HTTPS（建議用 tailscale serve）</span>';
  if (Notification.permission === 'granted') return '<span class="status-pill"><span class="dot ok"></span>議題完成時會通知</span>';
  return '<button class="btn small" type="button" id="notifyBtn">議題完成時通知我</button>';
}
function bindNotify(rerender) {
  const b = $('#notifyBtn');
  if (b) b.onclick = async () => { await Notification.requestPermission(); rerender(); };
}

export async function renderSettings() {
  if (!S.whoami.local) {
    view().innerHTML = `<section class="hero small"><p class="eyebrow">Settings · 設定</p><h1 class="display zh">這台裝置</h1>
      <p class="lede">已配對為「${esc(S.whoami.device || '手機')}」。模型路徑、保護門檻、排程與配對管理只能在電腦上修改。</p>
      <div class="toolbar">${notifyButton()}</div></section>`;
    bindNotify(renderSettings);
    return;
  }
  view().innerHTML = '<section class="hero small"><p class="eyebrow">Settings · 設定</p><h1 class="display zh">設定</h1></section><div id="setBody"><p class="lede">載入中…</p></div>';
  try {
    const [s, pairing, schedules, health] = await Promise.all([api('/api/settings'), api('/api/pairing'), api('/api/schedules'), api('/api/health')]);
    S.settings = s;
    paint(s, pairing, schedules, health);
  } catch (e) { $('#setBody').innerHTML = `<p class="lede">${esc(e.message)}</p>`; }
}

function scheduleRow(c) {
  const days = c.weekdays.length ? c.weekdays.map((d) => WEEK.find((w) => w[0] === d)?.[1]).join('、') : '每天';
  return `<div class="device"><span><b>${esc(c.name)}</b>（${esc(MODES[c.spec.mode]?.zh || c.spec.mode)}）<br><small class="muted">週${esc(days)} ${esc(c.time)} · 上次執行 ${esc(c.last_run || '—')}${c.enabled ? '' : ' · 已停用'}</small></span>
    <span class="toolbar"><button class="btn small quiet" type="button" data-run-schedule="${esc(c.id)}">立即執行</button><button class="btn small quiet" type="button" data-toggle-schedule="${esc(c.id)}">${c.enabled ? '停用' : '啟用'}</button><button class="btn small danger" type="button" data-del-schedule="${esc(c.id)}">刪除</button></span></div>`;
}

function paint(s, pr, schedules, health) {
  const real = S.providers.filter((p) => !p.mock);
  const ts = pr.tailscale;
  const d = s.digest;
  const userTemplates = S.templates.filter((t) => !t.builtin);
  const mcp = health.mcp_path;
  $('#setBody').innerHTML = `
  <section class="block">
    <div class="block-head"><span class="index">01 / Phone</span><h2>手機連線</h2><span class="aside">${pr.devices.length} 台已配對</span></div>
    <div class="settings-grid">
      <div class="stack">
        <span class="eyebrow">配對碼（${esc(until(Math.floor(pr.expires_at / 1000)).replace('重置', '失效'))}）</span>
        <div class="code-big" aria-live="polite">${esc(pr.code.slice(0, 3))} ${esc(pr.code.slice(3))}</div>
        <p class="muted" style="margin:0;font-size:.86rem">手機掃 QR code，或打開下面的網址再輸入這組數字。配對碼用過一次就會更換。</p>
        <div class="toolbar"><button class="btn small quiet" type="button" id="rotateBtn">換一組</button></div>
      </div>
      <div class="stack">
        ${pr.lan_url || (ts && ts.https_url) ? `<img class="qr" src="/api/pairing/qr.svg?t=${Date.now()}" alt="配對用 QR code" width="200" height="200">` : '<p class="muted">開啟區網連線或設定 Tailscale 後，這裡會出現 QR code。</p>'}
      </div>
      <div class="stack">
        <label class="check"><input type="checkbox" id="lanToggle"${pr.lan_enabled ? ' checked' : ''}> 允許同一個區網（Wi-Fi）的手機連線（連接埠 ${esc(pr.lan_port)}）</label>
        <dl class="kv">
          <dt>區網</dt><dd>${pr.lan_url ? `<span class="copyable" data-copy="${esc(pr.lan_url)}">${esc(pr.lan_url)}</span>` : '未開啟'}</dd>
          <dt>Tailscale</dt><dd>${ts ? `${esc(ts.ip)}${ts.dns ? ' · ' + esc(ts.dns) : ''}` : '沒有偵測到 Tailscale'}</dd>
          ${ts && ts.https_url ? `<dt>外出時</dt><dd>先在電腦執行 <span class="copyable" data-copy="${esc(ts.serve_command)}">${esc(ts.serve_command)}</span>，手機開 <span class="copyable" data-copy="${esc(ts.https_url)}">${esc(ts.https_url)}</span>（HTTPS：可安裝成 App、可收通知、可用語音輸入）</dd>` : ''}
        </dl>
        <span class="eyebrow">已配對的裝置</span>
        ${pr.devices.length ? pr.devices.map((x) => `<div class="device"><span>${esc(x.name)}<br><small class="muted">配對於 ${esc(stamp(x.created_at))} · 最近 ${esc(rel(x.last_seen))}</small></span><button class="btn small danger" type="button" data-revoke="${esc(x.id)}">撤銷</button></div>`).join('') : '<p class="muted" style="margin:0">還沒有</p>'}
      </div>
    </div>
  </section>

  <section class="block">
    <div class="block-head"><span class="index">02 / Digest</span><h2>每日技術情報</h2><span class="aside"><a href="#/digest">看今天的情報 ↗</a></span></div>
    <div class="settings-grid">
      <div class="stack">
        <label class="check"><input type="checkbox" id="dgEnabled"${d.enabled ? ' checked' : ''}> 每天自動整理</label>
        <label class="field"><span>時間</span><input type="time" id="dgTime" value="${esc(d.time)}"></label>
        <label class="check"><input type="checkbox" id="dgLlm"${d.use_llm ? ' checked' : ''}> 用 CLI 模型整理成摘要（會用到訂閱額度；關閉則只列清單）</label>
        <label class="field"><span>整理用的模型</span><select id="dgProvider">${providerOptions(d.provider || '', { allowEmpty: '自動（第一個就緒的 CLI）' })}</select></label>
        <label class="field"><span>模型（選填）</span><input type="text" id="dgModel" value="${esc(d.model || '')}" placeholder="留空用預設模型"></label>
      </div>
      <div class="stack">
        <span class="eyebrow">來源</span>
        <div class="flags">${SOURCES.map(([id, label]) => `<label class="check"><input type="checkbox" data-source="${id}"${d.sources.includes(id) ? ' checked' : ''}> ${esc(label)}</label>`).join('')}</div>
        <label class="field"><span>Reddit 看板（逗號分隔）</span><input type="text" id="dgSubs" value="${esc(d.subreddits.join(', '))}"></label>
      </div>
      <div class="stack">
        <label class="field"><span>你關注的主題（逗號分隔；用來加權排序與推薦閱讀）</span><textarea id="dgKeywords" style="min-height:6rem">${esc(d.keywords.join(', '))}</textarea></label>
        <label class="field"><span>每天最多幾則</span><input type="number" id="dgMax" min="10" max="80" value="${esc(d.max_items)}"></label>
      </div>
    </div>
  </section>

  <section class="block">
    <div class="block-head"><span class="index">03 / Schedules</span><h2>排程議題</h2><span class="aside">例如每晚雙模型審查；排程一律唯讀，工作坊只產出方案</span></div>
    <div id="scheduleList">${schedules.length ? schedules.map(scheduleRow).join('') : '<p class="muted">還沒有排程。</p>'}</div>
    <form id="scheduleForm" class="options">
      <label class="field"><span>名稱</span><input type="text" name="name" required placeholder="每晚審查今天的變更"></label>
      <label class="field"><span>模式</span><select name="mode">${MODE_ORDER.map((m) => `<option value="${m}">${esc(MODES[m].zh)}</option>`).join('')}</select></label>
      <label class="field"><span>時間</span><input type="time" name="time" value="22:00" required></label>
      <div class="field"><span class="field-label">星期（不勾＝每天）</span><div class="flags">${WEEK.map(([n, l]) => `<label class="check"><input type="checkbox" name="wd" value="${n}"> ${l}</label>`).join('')}</div></div>
      <label class="field wide-field"><span>議題</span><textarea name="prompt" required placeholder="請審查今天 git log 裡的變更，找出風險與遺漏的測試。" style="min-height:5rem"></textarea></label>
      <div class="field" style="align-self:end"><button class="btn primary" type="submit">新增排程</button></div>
    </form>
  </section>

  <section class="block">
    <div class="block-head"><span class="index">04 / Models</span><h2>預設模型</h2></div>
    <div class="settings-grid">
      ${real.map((p) => `<label class="field"><span>${esc(p.name)}</span><select data-default="${esc(p.id)}">${modelOptions(p.id, s.default_models[p.id] || '')}</select></label>`).join('')}
      <div class="field" style="align-self:end"><button class="btn small quiet" type="button" id="clearBad">清除「方案不支援」標記</button></div>
    </div>
  </section>

  <section class="block">
    <div class="block-head"><span class="index">05 / Guards</span><h2>保護機制</h2></div>
    <div class="settings-grid">
      <label class="field"><span>額度保護門檻（%）</span><input type="number" min="50" max="100" step="1" id="setQuota" value="${esc(s.quota_stop_percent)}"><small>任一時段用到這個比例就不再呼叫該模型</small></label>
      <div class="stack">
        <label class="check"><input type="checkbox" id="setReroute"${s.auto_reroute ? ' checked' : ''}> 額度不足時自動改派給另一個就緒的模型</label>
        <label class="check"><input type="checkbox" id="setPack"${s.context_pack ? ' checked' : ''}> 提案與討論時附上專案快照（檔案樹、README、最近 commit）</label>
        <label class="check" title="開啟時，Agora 驅動的 Claude／Codex 不會載入你個人的 MCP 工具（例如電腦操控）、外掛與 hooks"><input type="checkbox" id="setClean"${s.isolate_cli_config ? ' checked' : ''}> 隔離 CLI 的個人設定（不載入你的 MCP 工具、外掛、hooks）</label>
      </div>
      <label class="field"><span>單則發言逾時（秒）</span><input type="number" min="60" step="60" id="setTimeout" value="${esc(s.turn_timeout_secs)}"></label>
      <label class="field"><span>記憶壓縮門檻（字）</span><input type="number" min="2000" step="1000" id="setBudget" value="${esc(s.transcript_budget_chars)}"><small>帶入每則發言的對話超過這個長度就壓縮較早的內容</small></label>
    </div>
  </section>

  <section class="block">
    <div class="block-head"><span class="index">06 / Templates</span><h2>自訂範本</h2><span class="aside">在首頁按「把目前設定存成範本」新增</span></div>
    ${userTemplates.length ? userTemplates.map((t) => `<div class="device"><span><b>${esc(t.name)}</b> · ${esc(MODES[t.mode]?.zh || t.mode)}</span><button class="btn small danger" type="button" data-del-template="${esc(t.id)}">刪除</button></div>`).join('') : '<p class="muted">還沒有自訂範本。</p>'}
  </section>

  <section class="block">
    <div class="block-head"><span class="index">07 / CLI</span><h2>CLI 路徑</h2><span class="aside">留空 = 自動偵測</span></div>
    <div class="settings-grid">
      <label class="field"><span>claude.exe</span><input type="text" id="setClaude" value="${esc(s.claude_path || '')}" placeholder="${esc(providerById('claude')?.health?.executable || '自動偵測')}"></label>
      <label class="field"><span>codex.exe</span><input type="text" id="setCodex" value="${esc(s.codex_path || '')}" placeholder="${esc(providerById('codex')?.health?.executable || '自動偵測')}"></label>
    </div>
  </section>

  <section class="block">
    <div class="block-head"><span class="index">08 / MCP</span><h2>在 Claude Code／Codex 裡使用 Agora</h2></div>
    <p class="muted" style="margin-top:0">註冊之後，可以在 CLI 對話裡請它「用 agora_discuss 問另一個模型」或「讀 agora_digest」。透過 MCP 的議題一律唯讀。</p>
    ${mcp ? `<dl class="kv"><dt>Claude Code</dt><dd><span class="copyable" data-copy='claude mcp add agora -- "${esc(mcp)}"'>claude mcp add agora -- "${esc(mcp)}"</span></dd>
      <dt>Codex</dt><dd>在 <code>~/.codex/config.toml</code> 加上：<pre class="copyable" data-copy='[mcp_servers.agora]\ncommand = "${esc(mcp.replace(/\\/g, '\\\\'))}"'>[mcp_servers.agora]\ncommand = "${esc(mcp.replace(/\\/g, '\\\\'))}"</pre></dd></dl>`
      : '<p class="muted">找不到 agora-mcp.exe（請用 <code>cargo build -p agora-server --release</code> 編譯，它會和 agora-server.exe 放在同一個資料夾）。</p>'}
  </section>

  <section class="block">
    <div class="block-head"><span class="index">09 / Misc</span><h2>其他</h2></div>
    <div class="settings-grid">
      <label class="check"><input type="checkbox" id="setDemo"${s.demo_mode ? ' checked' : ''}> 示範模式：加入兩個離線模擬模型，不呼叫 CLI、不使用額度</label>
      <div class="field">${notifyButton()}</div>
      <dl class="kv"><dt>資料夾</dt><dd>${esc(s.data_dir)}</dd><dt>版本</dt><dd>${esc(S.whoami.version)}</dd></dl>
    </div>
    <div class="composer-foot"><button class="btn primary" type="button" id="saveSettings">儲存設定</button><span class="muted">配對、排程、範本的變更會立即生效；其他設定按「儲存設定」</span></div>
  </section>`;
  bind(s);
}

function list(v) {
  return v.split(/[,，\n]/).map((x) => x.trim()).filter(Boolean);
}

function bind(s) {
  bindNotify(renderSettings);
  const body = $('#setBody');
  body.addEventListener('click', async (e) => {
    const cp = e.target.closest('[data-copy]');
    if (cp) { toast((await copyText(cp.dataset.copy.replace(/\\n/g, '\n'))) ? '已複製' : '無法複製'); return; }
    const rv = e.target.closest('[data-revoke]');
    if (rv) {
      if (!confirm('撤銷後這台裝置需要重新配對。確定？')) return;
      try { await api(`/api/pairing/devices/${encodeURIComponent(rv.dataset.revoke)}`, { method: 'DELETE' }); renderSettings(); } catch (err) { toast(err.message, true); }
      return;
    }
    const run = e.target.closest('[data-run-schedule]');
    if (run) {
      try { const rec = await api(`/api/schedules/${encodeURIComponent(run.dataset.runSchedule)}/run`, { method: 'POST', body: {} }); location.hash = `#/s/${rec.id}`; } catch (err) { toast(err.message, true); }
      return;
    }
    const tg = e.target.closest('[data-toggle-schedule]');
    if (tg) {
      try {
        const all = await api('/api/schedules');
        const c = all.find((x) => x.id === tg.dataset.toggleSchedule);
        c.enabled = !c.enabled;
        await api('/api/schedules', { method: 'POST', body: c });
        renderSettings();
      } catch (err) { toast(err.message, true); }
      return;
    }
    const ds = e.target.closest('[data-del-schedule]');
    if (ds) {
      if (!confirm('刪除這個排程？')) return;
      try { await api(`/api/schedules/${encodeURIComponent(ds.dataset.delSchedule)}`, { method: 'DELETE' }); renderSettings(); } catch (err) { toast(err.message, true); }
      return;
    }
    const dt = e.target.closest('[data-del-template]');
    if (dt) {
      try { await api(`/api/templates/${encodeURIComponent(dt.dataset.delTemplate)}`, { method: 'DELETE' }); S.templates = await api('/api/templates'); renderSettings(); } catch (err) { toast(err.message, true); }
    }
  });
  $('#rotateBtn').onclick = async () => { try { await api('/api/pairing/rotate', { method: 'POST', body: {} }); renderSettings(); } catch (e) { toast(e.message, true); } };
  $('#lanToggle').onchange = async (e) => {
    try { await api('/api/network', { method: 'POST', body: { lan_enabled: e.target.checked } }); renderSettings(); toast(e.target.checked ? '已開啟區網連線' : '已關閉區網連線'); }
    catch (err) { e.target.checked = !e.target.checked; toast(err.message, true); }
  };
  $('#clearBad').onclick = async () => { try { await api('/api/settings', { method: 'PATCH', body: { clear_unsupported: true } }); await loadProviders(); renderSettings(); toast('已清除'); } catch (e) { toast(e.message, true); } };
  $('#scheduleForm').onsubmit = async (e) => {
    e.preventDefault();
    const f = e.target;
    const mode = f.mode.value;
    const C = pickProvider('claude'), G = pickProvider('gpt');
    const participants = (mode === 'solo' ? [C || G] : [C || G, G || C]).map((p) => ({ provider: p, model: null }));
    const spec = {
      project_id: S.projectId || S.projects[0]?.id, mode, participants, prompt: f.prompt.value, access: 'read_only',
      plan_only: mode === 'workshop', auto_approve: mode === 'workshop', rounds: mode === 'solo' ? null : (mode === 'relay' || mode === 'workshop' ? 1 : 2),
    };
    const weekdays = $$('input[name="wd"]:checked', f).map((x) => Number(x.value));
    try {
      await api('/api/schedules', { method: 'POST', body: { id: '', name: f.name.value, enabled: true, time: f.time.value, weekdays, spec, last_run: null } });
      toast('已新增排程');
      renderSettings();
    } catch (err) { toast(err.message, true); }
  };
  $('#saveSettings').onclick = async () => {
    const defaults = {};
    $$('[data-default]').forEach((sel) => { defaults[sel.dataset.default] = sel.value; });
    const digest = {
      enabled: $('#dgEnabled').checked,
      time: $('#dgTime').value || '08:00',
      use_llm: $('#dgLlm').checked,
      provider: $('#dgProvider').value || null,
      model: $('#dgModel').value.trim() || null,
      sources: $$('[data-source]').filter((x) => x.checked).map((x) => x.dataset.source),
      subreddits: list($('#dgSubs').value),
      keywords: list($('#dgKeywords').value),
      max_items: Number($('#dgMax').value) || 36,
    };
    const body = {
      quota_stop_percent: Number($('#setQuota').value),
      turn_timeout_secs: Number($('#setTimeout').value),
      transcript_budget_chars: Number($('#setBudget').value),
      auto_reroute: $('#setReroute').checked,
      context_pack: $('#setPack').checked,
      isolate_cli_config: $('#setClean').checked,
      claude_path: $('#setClaude').value,
      codex_path: $('#setCodex').value,
      demo_mode: $('#setDemo').checked,
      default_models: defaults,
      digest,
    };
    try {
      await api('/api/settings', { method: 'PATCH', body });
      S.whoami = await api('/api/whoami');
      await loadProviders(true);
      toast('設定已儲存');
      renderSettings();
    } catch (e) { toast(e.message, true); }
  };
}
