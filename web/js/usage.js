// Usage: per-provider quota cards, quota trend (small multiples by window), daily tokens
// (stacked by provider), per-mode table. Charts follow the dataviz rules: 2px lines, ≥8px
// end dots with a surface ring, 2px surface gaps, recessive hairline grid, legend for ≥2
// series, hover tooltips that never gate (table view below each chart).
import { $, api, compact, esc, num, rel, toast, until } from './util.js';
import { S, FAMILY, MODES, quotaOf, remaining, hp, meterLabel, providerById, renderMeters } from './state.js';

const view = () => $('#view');
let data = { history: [], stats: null };
let resizeTimer;

export function setQuotaInUsage(q) {
  S.quotas[q.provider] = q;
  renderMeters();
  if (S.route.startsWith('#/usage')) paintCards();
}

export async function renderUsage() {
  view().innerHTML = `<section class="hero small">
      <p class="eyebrow">Usage · 剩餘用量</p>
      <h1 class="display zh">還剩多少額度</h1>
      <p class="lede">數字直接來自 CLI，不經過任何第三方：Codex 由本機 app-server 免費查詢；Claude 從每次執行附帶的額度事件取得，所以要跑過一次才有數字。</p>
      <div class="toolbar"><button class="btn small" type="button" id="refreshAll">重新檢查全部</button><span class="muted" style="font-size:.85rem">每 5 分鐘自動更新</span></div>
    </section>
    <div class="usage-grid" id="usageGrid"></div>
    <section class="block">
      <div class="block-head"><span class="index">01 / Trend</span><h2>額度走勢（過去 7 天）</h2><span class="aside">已用比例，越高越接近上限</span></div>
      <div class="chart-row" id="trend"></div>
    </section>
    <section class="block">
      <div class="block-head"><span class="index">02 / Spend</span><h2>Agora 用掉的 tokens（過去 30 天）</h2></div>
      <div class="tiles" id="tiles"></div>
      <div id="daily"></div>
    </section>
    <section class="block">
      <div class="block-head"><span class="index">03 / Modes</span><h2>各模式表現</h2><span class="aside">「已確認」比例＝結論經驗證指令證實的議題占比</span></div>
      <div id="modes"></div>
    </section>`;
  $('#refreshAll').onclick = async (e) => {
    e.target.disabled = true;
    try {
      S.providers = await api('/api/providers/refresh', { method: 'POST', body: {} });
      S.providers.forEach((p) => { if (p.quota) S.quotas[p.id] = p.quota; });
      paintCards(); renderMeters(); toast('已重新檢查');
    } catch (err) { toast(err.message, true); } finally { e.target.disabled = false; }
  };
  paintCards();
  try {
    const [history, stats] = await Promise.all([api('/api/usage/history?days=7'), api('/api/usage/stats?days=30')]);
    data = { history, stats };
    paintCharts();
  } catch (e) { $('#trend').innerHTML = `<p class="muted">${esc(e.message)}</p>`; }
  window.onresize = () => { clearTimeout(resizeTimer); resizeTimer = setTimeout(() => { if (S.route.startsWith('#/usage')) paintCharts(); }, 150); };
}

function paintCards() {
  const grid = $('#usageGrid');
  if (!grid) return;
  const list = [...S.providers].sort((a, b) => Number(a.mock) - Number(b.mock));
  grid.innerHTML = list.map((p) => {
    const q = quotaOf(p.id);
    const rem = remaining(q);
    const h = p.health;
    const wins = q && q.windows.length
      ? q.windows.map((w) => `<div class="win"><span>${esc(w.label)}</span><span class="bar" aria-hidden="true"><i style="width:${Math.min(100, w.used_percent).toFixed(1)}%"></i></span><span>已用 ${Math.round(w.used_percent)}%<span class="reset"> · ${esc(until(w.resets_at))}</span></span></div>`).join('')
      : `<p class="muted" style="margin:0">${p.family === 'claude' ? '還沒有資料：Claude 的額度會在下一次執行時一併回報。' : '還沒有資料。'}</p>`;
    const action = p.mock ? '' : p.id === 'claude'
      ? '<button class="btn small quiet" type="button" data-probe="claude" title="送一個極短的請求給 haiku，用掉極少量額度">主動更新（用極少額度）</button>'
      : '<button class="btn small quiet" type="button" data-refresh="1">重新整理（免費）</button>';
    return `<article class="usage-card fam-${p.family}">
      <header><span class="eyebrow">${esc(meterLabel(p))}${q && q.plan ? ' · ' + esc(String(q.plan).toUpperCase()) : ''}</span><h2>${esc(p.name)}</h2></header>
      <div class="health"><span class="dot ${h ? (h.ok ? 'ok' : 'bad') : ''}"></span>${h ? esc(h.summary) : '檢查中…'}</div>
      ${h && !h.ok && h.fix ? `<div class="fix">${esc(h.fix)}</div>` : ''}
      <div><div class="big-num">${rem == null ? '—' : Math.round(rem) + '%'}<small>剩餘（取最緊的時段）</small></div>${hp(rem, true)}</div>
      <div>${wins}</div>
      <div class="meta">${q ? `<span>來源：${esc(q.source)}</span><span>觀測：${esc(rel(q.observed_at))}</span>` : ''}${h && h.version ? `<span>版本：${esc(h.version)}</span>` : ''}${h && h.account ? `<span>帳號：${esc(h.account)}</span>` : ''}</div>
      <div class="toolbar">${action}</div>
    </article>`;
  }).join('');
  grid.onclick = async (e) => {
    const probe = e.target.closest('[data-probe]');
    const refresh = e.target.closest('[data-refresh]');
    if (!probe && !refresh) return;
    const btn = probe || refresh;
    if (probe && !confirm('會送一個極短的請求給 Claude haiku（用掉極少量訂閱額度）來取得最新額度。繼續？')) return;
    btn.disabled = true;
    try {
      if (probe) { const r = await api('/api/usage/probe-claude', { method: 'POST', body: {} }); if (r.quota) setQuotaInUsage(r.quota); }
      else { (await api('/api/usage/refresh', { method: 'POST', body: {} })).forEach(setQuotaInUsage); }
      toast('已更新');
    } catch (err) { toast(err.message, true); } finally { btn.disabled = false; }
  };
}

// ------------------------------------------------------------------ charts

const seriesVar = (provider) => `var(--series-${(providerById(provider)?.family || (provider.includes('claude') ? 'claude' : 'gpt'))})`;
const seriesName = (provider) => {
  const p = providerById(provider);
  return p ? meterLabel(p) : provider;
};

function tooltip() {
  let t = $('#chartTip');
  if (!t) { t = document.createElement('div'); t.id = 'chartTip'; t.className = 'chart-tip'; t.hidden = true; document.body.appendChild(t); }
  return t;
}

function showTip(evt, title, rows) {
  const t = tooltip();
  t.replaceChildren();
  const h = document.createElement('div');
  h.className = 'tip-title';
  h.textContent = title;
  t.appendChild(h);
  for (const r of rows) {
    const row = document.createElement('div');
    row.className = 'tip-row';
    const key = document.createElement('i');
    key.style.background = r.color;
    const v = document.createElement('b');
    v.textContent = r.value;
    const n = document.createElement('span');
    n.textContent = r.name;
    row.append(key, v, n);
    t.appendChild(row);
  }
  t.hidden = false;
  const x = Math.min(window.innerWidth - t.offsetWidth - 12, evt.clientX + 14);
  t.style.left = `${x + window.scrollX}px`;
  t.style.top = `${evt.clientY + window.scrollY - t.offsetHeight - 10}px`;
}
const hideTip = () => { const t = $('#chartTip'); if (t) t.hidden = true; };

const fmtTime = (ms) => { const d = new Date(ms); return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`; };

function lineChart(el, title, series, { yMax = 100, unit = '%' } = {}) {
  const W = Math.max(280, el.clientWidth || 480), H = 220, L = 40, R = 46, T = 12, B = 28;
  const all = series.flatMap((s) => s.points);
  if (!all.length) { el.innerHTML = `<div class="chart-card"><h3>${esc(title)}</h3><p class="muted">還沒有足夠的資料。</p></div>`; return; }
  // The domain is always the whole window the title promises (past 7 days), not just the span
  // that happens to have samples.
  const t1 = Math.max(Date.now(), ...all.map((p) => p.t));
  const t0 = Math.min(t1 - 7 * 86_400_000, ...all.map((p) => p.t));
  const x = (t) => L + ((t - t0) / (t1 - t0)) * (W - L - R);
  const y = (v) => T + (1 - Math.min(v, yMax) / yMax) * (H - T - B);
  const grid = [0, 25, 50, 75, 100].map((g) => `<line class="grid" x1="${L}" x2="${W - R}" y1="${y(g)}" y2="${y(g)}"/><text class="tick" x="${L - 6}" y="${y(g) + 4}" text-anchor="end">${g}${unit}</text>`).join('');
  const days = [];
  for (let d = new Date(t0); d.getTime() <= t1; d.setHours(24, 0, 0, 0)) days.push(new Date(d));
  const xt = days.slice(1).filter((_, i, a) => a.length <= 8 || i % 2 === 0).map((d) => `<text class="tick" x="${x(d.getTime())}" y="${H - 8}" text-anchor="middle">${d.getMonth() + 1}/${d.getDate()}</text>`).join('');
  const lines = series.map((s) => {
    const pts = s.points.map((p) => `${x(p.t).toFixed(1)},${y(p.v).toFixed(1)}`).join(' ');
    const last = s.points[s.points.length - 1];
    return `<polyline class="series" points="${pts}" style="stroke:${s.color}"/><circle class="end" cx="${x(last.t)}" cy="${y(last.v)}" r="4" style="fill:${s.color}"/>`;
  }).join('');
  // Direct end values, only when they don't collide.
  const ends = series.map((s) => ({ s, p: s.points[s.points.length - 1] })).sort((a, b) => y(a.p.v) - y(b.p.v));
  const collide = ends.some((e, i) => i > 0 && Math.abs(y(e.p.v) - y(ends[i - 1].p.v)) < 13);
  const labels = collide ? '' : ends.map((e) => `<text class="end-label" x="${x(e.p.t) + 8}" y="${y(e.p.v) + 4}">${Math.round(e.p.v)}${unit}</text>`).join('');
  const legend = series.length > 1 ? `<div class="legend">${series.map((s) => `<span><i class="line-key" style="background:${s.color}"></i>${esc(s.name)}</span>`).join('')}</div>` : '';
  const rows = series.flatMap((s) => s.points.map((p) => ({ name: s.name, t: p.t, v: p.v }))).sort((a, b) => a.t - b.t);
  el.innerHTML = `<div class="chart-card"><h3>${esc(title)}${series.length === 1 ? ` · ${esc(series[0].name)}` : ''}</h3>${legend}
    <svg class="chart" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="${esc(title)}">${grid}${xt}${lines}${labels}
      <line class="cross" x1="0" x2="0" y1="${T}" y2="${H - B}" visibility="hidden"/><rect class="hit" x="${L}" y="${T}" width="${W - L - R}" height="${H - T - B}"/></svg>
    <details class="table-view"><summary>表格檢視</summary><table><thead><tr><th>時間</th><th>系列</th><th>已用</th></tr></thead><tbody>${rows.map((r) => `<tr><td>${esc(fmtTime(r.t))}</td><td>${esc(r.name)}</td><td>${r.v.toFixed(1)}${unit}</td></tr>`).join('')}</tbody></table></details></div>`;
  const svg = el.querySelector('svg');
  const cross = svg.querySelector('.cross');
  const hit = svg.querySelector('.hit');
  hit.addEventListener('pointermove', (evt) => {
    const box = svg.getBoundingClientRect();
    const px = ((evt.clientX - box.left) / box.width) * W;
    const t = t0 + ((px - L) / (W - L - R)) * (t1 - t0);
    const near = series.map((s) => ({ s, p: s.points.reduce((a, b) => (Math.abs(b.t - t) < Math.abs(a.t - t) ? b : a)) }));
    const snapT = near.reduce((a, b) => (Math.abs(b.p.t - t) < Math.abs(a.p.t - t) ? b : a)).p.t;
    cross.setAttribute('x1', x(snapT)); cross.setAttribute('x2', x(snapT)); cross.setAttribute('visibility', 'visible');
    showTip(evt, fmtTime(snapT), near.map((n) => ({ color: n.s.color, value: `${n.p.v.toFixed(1)}${unit}`, name: n.s.name })));
  });
  hit.addEventListener('pointerleave', () => { cross.setAttribute('visibility', 'hidden'); hideTip(); });
}

function columnChart(el, days) {
  const W = Math.max(280, el.clientWidth || 640), H = 240, L = 48, R = 12, T = 12, B = 28;
  if (!days.length) { el.innerHTML = '<p class="muted">過去 30 天還沒有議題。</p>'; return; }
  const providers = [...new Set(days.flatMap((d) => Object.keys(d.by_provider || {})))].sort();
  const totals = days.map((d) => Object.values(d.by_provider || {}).reduce((a, b) => a + b, 0));
  const max = Math.max(...totals, 1);
  const step = Math.pow(10, Math.floor(Math.log10(max)));
  const top = Math.ceil(max / step) * step;
  const y = (v) => T + (1 - v / top) * (H - T - B);
  const band = (W - L - R) / days.length;
  const bw = Math.min(24, band * 0.6);
  const ticks = [0, 0.5, 1].map((f) => `<line class="grid" x1="${L}" x2="${W - R}" y1="${y(top * f)}" y2="${y(top * f)}"/><text class="tick" x="${L - 6}" y="${y(top * f) + 4}" text-anchor="end">${compact(top * f)}</text>`).join('');
  let bars = '';
  days.forEach((d, i) => {
    const cx = L + band * i + band / 2;
    let acc = 0;
    providers.forEach((p, k) => {
      const v = (d.by_provider || {})[p] || 0;
      if (!v) return;
      const y0 = y(acc), y1 = y(acc + v);
      const h = Math.max(1, y0 - y1 - (acc > 0 ? 2 : 0));
      const isTop = providers.slice(k + 1).every((q) => !((d.by_provider || {})[q]));
      bars += `<path class="bar" data-i="${i}" data-p="${esc(p)}" d="${roundedTop(cx - bw / 2, y1, bw, h, isTop ? 4 : 0)}" style="fill:${seriesVar(p)}"/>`;
      acc += v;
    });
  });
  const labelsX = days.map((d, i) => (days.length <= 10 || i % Math.ceil(days.length / 8) === 0 ? `<text class="tick" x="${L + band * i + band / 2}" y="${H - 8}" text-anchor="middle">${esc(d.date.slice(5).replace('-', '/'))}</text>` : '')).join('');
  const legend = providers.length > 1 ? `<div class="legend">${providers.map((p) => `<span><i class="swatch" style="background:${seriesVar(p)}"></i>${esc(seriesName(p))}</span>`).join('')}</div>` : '';
  el.innerHTML = `<div class="chart-card"><h3>每日 tokens（輸入＋輸出）${providers.length === 1 ? ` · ${esc(seriesName(providers[0]))}` : ''}</h3>${legend}
    <svg class="chart" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="每日 tokens">${ticks}${bars}${labelsX}</svg>
    <details class="table-view"><summary>表格檢視</summary><table><thead><tr><th>日期</th><th>議題</th><th>發言</th><th>輸入</th><th>輸出</th></tr></thead><tbody>${days.map((d) => `<tr><td>${esc(d.date)}</td><td>${d.sessions}</td><td>${d.turns}</td><td>${num(d.input_tokens)}</td><td>${num(d.output_tokens)}</td></tr>`).join('')}</tbody></table></details></div>`;
  el.querySelectorAll('.bar').forEach((b) => {
    b.addEventListener('pointermove', (evt) => {
      const d = days[Number(b.dataset.i)];
      showTip(evt, d.date, providers.filter((p) => d.by_provider[p]).map((p) => ({ color: seriesVar(p), value: compact(d.by_provider[p]), name: seriesName(p) })));
    });
    b.addEventListener('pointerleave', hideTip);
  });
}

function roundedTop(x, y, w, h, r) {
  r = Math.min(r, w / 2, h);
  return `M${x},${y + h}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + h}Z`;
}

function paintCharts() {
  const trend = $('#trend');
  if (!trend) return;
  const byWindow = { five_hour: '5 小時窗', weekly: '每週窗' };
  const providersWithData = [...new Set(data.history.map((h) => h.provider))];
  const realOnly = providersWithData.some((p) => !providerById(p)?.mock);
  const provs = providersWithData.filter((p) => !realOnly || !providerById(p)?.mock);
  trend.innerHTML = Object.keys(byWindow).map((k) => `<div class="chart-slot" data-window="${k}"></div>`).join('');
  for (const [key, label] of Object.entries(byWindow)) {
    const series = provs.map((p) => ({
      name: seriesName(p),
      color: seriesVar(p),
      points: data.history.filter((h) => h.provider === p && h.used[key] != null).map((h) => ({ t: h.t, v: h.used[key] })),
    })).filter((s) => s.points.length);
    lineChart(trend.querySelector(`[data-window="${key}"]`), label, series);
  }

  const st = data.stats;
  if (!st) return;
  const sum = (f) => st.by_day.reduce((a, d) => a + f(d), 0);
  const confirmed = st.by_mode.reduce((a, m) => a + m.confirmed, 0);
  const withVerdict = st.by_mode.reduce((a, m) => a + m.with_verdict, 0);
  $('#tiles').innerHTML = [
    ['議題', num(sum((d) => d.sessions))],
    ['模型發言', num(sum((d) => d.turns))],
    ['Tokens', compact(sum((d) => d.input_tokens + d.output_tokens))],
    ['已確認比例', withVerdict ? `${Math.round((confirmed / withVerdict) * 100)}%` : '—'],
  ].map(([l, v]) => `<div class="tile"><span>${esc(l)}</span><b>${esc(v)}</b></div>`).join('');
  columnChart($('#daily'), st.by_day);
  $('#modes').innerHTML = st.by_mode.length ? `<table class="data"><thead><tr><th>模式</th><th>議題</th><th>發言</th><th>平均每題 tokens</th><th>已確認比例</th></tr></thead><tbody>${st.by_mode.map((m) => {
    const name = MODES[m.mode]?.zh || m.mode;
    const avg = m.sessions ? (m.input_tokens + m.output_tokens) / m.sessions : 0;
    return `<tr><td>${esc(name)}</td><td>${m.sessions}</td><td>${m.turns}</td><td>${compact(Math.round(avg))}</td><td>${m.with_verdict ? `${Math.round((m.confirmed / m.with_verdict) * 100)}%` : '—'}</td></tr>`;
  }).join('')}</tbody></table>` : '<p class="muted">過去 30 天還沒有議題。</p>';
}

export { FAMILY };
