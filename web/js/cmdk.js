// Accessible command palette (Ctrl/Cmd+K).
import { api, esc } from './util.js';
import { motionSetting, setMotionSetting } from './motion.js';
import { openShortcuts } from './shortcuts.js';

let dialog;
let input;
let list;
let opener;
let active = 0;
let commands = [];

const base = () => [
  { id: 'home', label: '前往議題', search: 'sessions 議題 session', run: () => { location.hash = '#/sessions'; } },
  { id: 'team', label: '前往團隊', search: 'team 專案 工作流 workflow home 首頁', run: () => { location.hash = '#/'; } },
  { id: 'digest', label: '前往情報', search: 'digest 技術情報', run: () => { location.hash = '#/digest'; } },
  { id: 'usage', label: '前往用量', search: 'usage 配額', run: () => { location.hash = '#/usage'; } },
  { id: 'phone', label: '手機連線（步驟、QR code、配對碼）', search: 'phone mobile iphone android tailscale qr 手機 配對 連線', run: () => { location.hash = '#/settings/phone'; } },
  { id: 'settings', label: '前往設定', search: 'settings preferences', run: () => { location.hash = '#/settings'; } },
  { id: 'new', label: '開新議題', search: 'new session prompt 新增 開始', run: () => {
    location.hash = '#/sessions';
    const focus = () => document.querySelector('#prompt')?.focus();
    setTimeout(focus, 0); setTimeout(focus, 250);
  } },
  { id: 'theme', label: '切換主題', search: 'theme light dark 自動 淺色 深色', run: () => document.dispatchEvent(new CustomEvent('agora:cycletheme')) },
  { id: 'motion', label: `切換動態效果（目前：${{ auto: '自動', on: '開', off: '關' }[motionSetting()] || '自動'}）`, search: 'motion animation 動畫 自動 開 關', run: () => {
    const values = ['auto', 'on', 'off'];
    setMotionSetting(values[(values.indexOf(motionSetting()) + 1) % values.length]);
  } },
  { id: 'shortcuts', label: '開啟快捷鍵說明', search: 'keyboard help hotkey ?', run: openShortcuts },
];

function visible() {
  const q = input.value.trim().toLocaleLowerCase();
  return commands.filter((item) => `${item.label} ${item.search || ''}`.toLocaleLowerCase().includes(q));
}

function paint() {
  const items = visible();
  active = Math.max(0, Math.min(active, items.length - 1));
  list.innerHTML = items.length ? items.map((item, index) => `<li id="cmdk-${esc(item.id)}" role="option" aria-selected="${index === active}" data-cmd="${esc(item.id)}"><span>${esc(item.label)}</span>${item.meta ? `<small>${esc(item.meta)}</small>` : ''}</li>`).join('') : '<li class="cmdk-empty">沒有符合的指令</li>';
  input.setAttribute('aria-activedescendant', items[active] ? `cmdk-${items[active].id}` : '');
  list.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
}

function run(id) {
  const item = commands.find((command) => command.id === id);
  if (!item) return;
  dialog.close();
  item.run();
}

async function open() {
  ensure();
  if (dialog.open) { dialog.close(); return; }
  opener = document.activeElement;
  commands = base(); active = 0; input.value = ''; paint();
  dialog.showModal(); input.focus();
  try {
    const [sessions, workflows] = await Promise.all([api('/api/sessions'), api('/api/workflows')]);
    commands.push(...workflows.slice(0, 10).map((workflow, index) => ({
      id: `workflow-${index}`,
      label: workflow.title || '未命名工作流',
      meta: workflow.status || '',
      search: `最近 工作流 團隊 ${workflow.status || ''}`,
      run: () => { location.hash = `#/w/${workflow.id}`; },
    })), ...sessions.slice(0, 20).map((session, index) => ({
      id: `session-${index}`,
      label: session.title || '未命名議題',
      meta: session.status || '',
      search: `最近 議題 ${session.status || ''}`,
      run: () => { location.hash = `#/s/${session.id}`; },
    })));
    if (dialog.open) paint();
  } catch { /* static commands remain available */ }
}

function ensure() {
  if (dialog) return;
  dialog = document.createElement('dialog');
  dialog.className = 'cmdk-dialog';
  dialog.setAttribute('aria-labelledby', 'cmdkTitle');
  dialog.innerHTML = `<div class="cmdk-shell"><h2 id="cmdkTitle" class="sr">命令面板</h2><label class="sr" for="cmdkInput">搜尋指令</label><input id="cmdkInput" type="search" role="combobox" aria-autocomplete="list" aria-controls="cmdkList" aria-expanded="true" placeholder="搜尋指令或最近議題…" autocomplete="off"><ul id="cmdkList" role="listbox"></ul><p class="cmdk-hint"><kbd>↑</kbd><kbd>↓</kbd> 選擇　<kbd>Enter</kbd> 執行　<kbd>Esc</kbd> 關閉</p></div>`;
  document.body.appendChild(dialog);
  input = dialog.querySelector('#cmdkInput'); list = dialog.querySelector('#cmdkList');
  input.addEventListener('input', () => { active = 0; paint(); });
  input.addEventListener('keydown', (e) => {
    const items = visible();
    if (e.key === 'ArrowDown') { e.preventDefault(); active = items.length ? (active + 1) % items.length : 0; paint(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); active = items.length ? (active - 1 + items.length) % items.length : 0; paint(); }
    else if (e.key === 'Enter' && items[active]) { e.preventDefault(); run(items[active].id); }
  });
  list.addEventListener('click', (e) => { const option = e.target.closest('[data-cmd]'); if (option) run(option.dataset.cmd); });
  list.addEventListener('mousemove', (e) => { const option = e.target.closest('[data-cmd]'); if (!option) return; active = visible().findIndex((item) => item.id === option.dataset.cmd); paint(); });
  dialog.addEventListener('close', () => {
    const target = opener && document.contains(opener) ? opener : document.querySelector('#view h1');
    target?.focus?.({ preventScroll: true }); opener = null;
  });
}

export function initCommandPalette() {
  document.addEventListener('keydown', (e) => {
    if (e.isComposing || e.altKey || !(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== 'k') return;
    e.preventDefault(); open();
  });
  document.getElementById('cmdkBtn')?.addEventListener('click', open);
}
