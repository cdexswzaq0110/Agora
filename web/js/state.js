// App state, vocabulary and the provider/quota helpers shared by every view.
import { $, api, esc, store } from './util.js';

export const MODES = {
  workshop: { en: 'WORKSHOP', zh: '方案工作坊', desc: '兩個模型各自提案、互評，整合成附驗收標準的方案；你核准後才在隔離分支實作、驗證、依標準審查。' },
  solo: { en: 'SOLO', zh: '單人', desc: '一個模型直接回答或動手做。最快、最省額度。' },
  relay: { en: 'RELAY', zh: '協做', desc: '一個執行、一個審查，可接驗證指令；審查不過就修訂，修訂次數有上限。' },
  roundtable: { en: 'ROUNDTABLE', zh: '討論', desc: '第一輪各自獨立作答，避免互相帶風向；之後互相回應，最後整理共識與分歧。' },
  debate: { en: 'DEBATE', zh: '辯論', desc: '正反方攻防，雙方模型交叉裁判；裁判意見不一致時直接標示分歧。' },
};
export const MODE_ORDER = ['workshop', 'solo', 'relay', 'roundtable', 'debate'];
export const ROLE_ZH = {
  solo: '單人', builder: '執行者', reviewer: '審查者', panelist: '與談人', synthesizer: '總結者', proponent: '正方', opponent: '反方',
  judge: '裁判', planner: '提案者', critic: '評論者', summarizer: '情報編輯', probe: '探測',
};
export const STATUS_ZH = { running: '進行中', awaiting_approval: '等待核准', done: '完成', failed: '失敗', cancelled: '已取消' };
export const FAMILY = { claude: 'Claude', gpt: 'GPT' };
export const WORKSHOP_STEPS = ['提案', '互評', '整合', '等待核准', '實作', '驗證', '審查'];

export const S = {
  whoami: null,
  providers: [],
  projects: [],
  templates: [],
  quotas: {},
  settings: null,
  projectId: store.get('project', null),
  desktop: new URLSearchParams(location.search).get('shell') === 'desktop' || store.get('desktop', false),
  rec: null,
  es: null,
  expanded: new Set(),
  route: '',
};
if (new URLSearchParams(location.search).get('shell') === 'desktop') store.set('desktop', true);

export const defaultComposer = () => ({
  mode: 'workshop',
  cast: { solo: [], relay: [], roundtable: [], debate: [], workshop: [] },
  judge: { roundtable: '', debate: '', workshop: '' },
  builder: '',
  reviewer: '',
  rounds: { relay: 1, roundtable: 2, debate: 2, workshop: 1 },
  access: 'read_only',
  verify: '',
  maxTurns: '',
  prompt: '',
  blind: false,
  redTeam: true,
  planOnly: false,
  autoApprove: false,
  isolate: true,
  template: null,
  seed: null,
  compare: [],
});
const saved = store.get('composer', {});
S.c = Object.assign(defaultComposer(), saved);
S.c.cast = Object.assign(defaultComposer().cast, saved.cast || {});
S.c.judge = Object.assign(defaultComposer().judge, saved.judge || {});
S.c.rounds = Object.assign(defaultComposer().rounds, saved.rounds || {});
export const saveComposer = () => store.set('composer', S.c);

export const providerById = (id) => S.providers.find((p) => p.id === id);
export const famOf = (id) => (providerById(id) || {}).family || (String(id).includes('claude') ? 'claude' : 'gpt');
export const quotaOf = (id) => S.quotas[id] || (providerById(id) || {}).quota || null;

export function remaining(q) {
  if (!q || !q.windows || !q.windows.length) return null;
  return Math.max(0, 100 - Math.max(...q.windows.map((w) => w.used_percent)));
}

export function pickProvider(family) {
  const list = S.providers.filter((p) => p.family === family);
  // Demo mode promises "no CLI, no quota" (settings label, mock.rs): default to the family's mock even
  // when the real CLI is healthy. Real CLIs stay selectable by hand. See docs/lessons/0009.
  const mock = S.whoami?.demo && list.find((p) => p.mock);
  return (mock || list.find((p) => p.health && p.health.ok) || list.find((p) => !p.mock) || list[0] || {}).id || '';
}

export function ensureCast() {
  const C = pickProvider('claude'), G = pickProvider('gpt');
  const fix = (slots, wanted) => {
    while (slots.length < wanted.length) slots.push({ provider: wanted[slots.length], model: '' });
    slots.forEach((s, i) => { if (!providerById(s.provider)) { s.provider = wanted[i] || C || G; s.model = ''; } });
  };
  fix(S.c.cast.solo, [C || G]);
  fix(S.c.cast.relay, [C || G, G || C]);
  fix(S.c.cast.roundtable, [C || G, G || C]);
  fix(S.c.cast.debate, [C || G, G || C]);
  fix(S.c.cast.workshop, [C || G, G || C]);
  S.c.cast.solo.length = 1;
}

export function hpClass(rem) {
  if (rem == null) return 'x';
  if (rem > 60) return 'g';
  if (rem > 40) return 'y';
  if (rem > 20) return 'o';
  return 'r';
}

export function hp(rem, big = false) {
  const on = rem == null ? 0 : Math.ceil(Math.max(0, rem) / 20);
  const label = rem == null ? '剩餘未知' : `剩餘 ${Math.round(rem)}%`;
  return `<span class="hp ${hpClass(rem)}${big ? ' big' : ''}" role="img" aria-label="${label}">${[0, 1, 2, 3, 4].map((i) => `<i class="${i < on ? 'on' : ''}"></i>`).join('')}</span>`;
}

export const meterLabel = (p) => (p.mock ? '模擬 ' : '') + (FAMILY[p.family] || p.family).toUpperCase();

export function renderMeters() {
  const el = $('#meters');
  if (!el) return;
  const realReady = S.providers.some((p) => !p.mock && p.health && p.health.ok);
  const list = S.providers.filter((p) => !p.mock || (S.whoami?.demo && !realReady));
  el.innerHTML = list.map((p) => {
    const rem = remaining(quotaOf(p.id));
    return `<a class="meter" href="#/usage" title="${esc(p.name)}：${rem == null ? '尚無資料' : `剩餘約 ${Math.round(rem)}%`}"><b>${esc(meterLabel(p))}</b>${hp(rem)}<em>${rem == null ? '—' : Math.round(rem) + '%'}</em></a>`;
  }).join('');
}

export async function loadProviders(refresh = false) {
  S.providers = await api(refresh ? '/api/providers/refresh' : '/api/providers', refresh ? { method: 'POST', body: {} } : {});
  S.providers.forEach((p) => { if (p.quota) S.quotas[p.id] = p.quota; });
  renderMeters();
}

export function providerOptions(selected, { allowEmpty = '' } = {}) {
  const opts = S.providers.map((p) => {
    const ready = p.health ? (p.health.ok ? '' : '（未就緒）') : '';
    return `<option value="${esc(p.id)}"${p.id === selected ? ' selected' : ''}>${esc(p.name)}${ready}</option>`;
  }).join('');
  return (allowEmpty ? `<option value="">${esc(allowEmpty)}</option>` : '') + opts;
}

export const EFFORT_ZH = { low: '低', medium: '中', high: '高', xhigh: '很高', max: '最高', ultra: '極限' };

/**
 * Efforts a provider/model accepts — the same rule as the server's `supported_efforts`:
 * the model's entry, else the CLI-default entry (`id: ""`, the intersection of all models).
 */
export function effortsFor(pid, model) {
  const p = providerById(pid);
  if (!p) return { list: [], def: null };
  const m = p.models.find((x) => x.id === (model || '')) || p.models.find((x) => x.id === '');
  return { list: m?.efforts || [], def: m?.default_effort || null };
}

/** "模型預設（low）" only when the CLI reports that model's default; otherwise say so. */
export function effortOptions(pid, model, selected) {
  const p = providerById(pid);
  const { list, def } = effortsFor(pid, model);
  if (!list.length) return '<option value="">這個模型無法調整</option>';
  const known = def && (model || p?.mock);
  return `<option value="">${known ? `模型預設（${esc(def)}）` : '模型預設（由 CLI 決定）'}</option>`
    + list.map((e) => `<option value="${esc(e)}"${e === selected ? ' selected' : ''}>${esc(e)}（${esc(EFFORT_ZH[e] || e)}）</option>`).join('');
}

export function modelOptions(pid, selected) {
  const p = providerById(pid);
  if (!p) return '';
  const bad = new Set(p.unsupported_models || []);
  const opts = p.models.map((m) => {
    const label = m.id === '' && p.default_model ? `預設（${p.default_model}）` : m.label;
    const defaultRejected = m.id === '' && !p.default_model && [...bad].some((b) => m.label.includes(`（${b}）`));
    const flag = bad.has(m.id) || defaultRejected ? '（方案不支援）' : '';
    return `<option value="${esc(m.id)}"${m.id === selected ? ' selected' : ''}${bad.has(m.id) && m.id !== selected ? ' disabled' : ''}>${esc(label)}${flag}</option>`;
  });
  if (selected && !p.models.some((m) => m.id === selected)) opts.push(`<option value="${esc(selected)}" selected>${esc(selected)}</option>`);
  return opts.join('');
}

export const chip = (p) => `<span class="chip fam-${famOf(p.provider)}">${esc(FAMILY[famOf(p.provider)])}${p.model ? ' · ' + esc(p.model) : ''}</span>`;
