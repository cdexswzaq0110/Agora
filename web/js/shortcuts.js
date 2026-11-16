// 鍵盤快捷鍵 (v0.4, plan H): `?` opens the help dialog; `g` then h/d/u/s changes page.
// Single-character shortcuts must be switchable off (WCAG 2.1.4) — a per-device preference in
// localStorage, toggled in the dialog and in 設定. Never fires while typing, composing (IME),
// with Ctrl/Alt/Meta held, or while a dialog is open.
import { $, store } from './util.js';

const PAGES = { h: ['#/', '議題'], d: ['#/digest', '情報'], u: ['#/usage', '用量'], s: ['#/settings', '設定'] };
const SEQ_MS = 1500;
let pendingG = 0;
let dialog = null;
let opener = null;

export const shortcutsEnabled = () => store.get('shortcuts', true) !== false;

export function setShortcutsEnabled(on) {
  store.set('shortcuts', Boolean(on));
  const box = dialog?.querySelector('#scToggle');
  if (box) box.checked = Boolean(on);
  const settingsBox = document.getElementById('setShortcuts');
  if (settingsBox) settingsBox.checked = Boolean(on);
}

function typingTarget(el) {
  if (!el || el === document.body) return false;
  if (el.isContentEditable) return true;
  const tag = el.tagName;
  if (tag === 'TEXTAREA' || tag === 'SELECT') return true;
  if (tag !== 'INPUT') return false;
  return !['checkbox', 'radio', 'button', 'submit', 'reset', 'range', 'color', 'file'].includes((el.type || '').toLowerCase());
}

function ensureDialog() {
  if (dialog) return dialog;
  dialog = document.createElement('dialog');
  dialog.className = 'shortcuts-dialog';
  dialog.setAttribute('aria-labelledby', 'scTitle');
  dialog.innerHTML = `<form method="dialog">
      <h2 id="scTitle">鍵盤快捷鍵</h2>
      <dl class="sc-list">
        <dt><kbd>?</kbd></dt><dd>開關這個說明</dd>
        ${Object.entries(PAGES).map(([k, [, zh]]) => `<dt><kbd>g</kbd> <kbd>${k}</kbd></dt><dd>前往「${zh}」</dd>`).join('')}
        <dt><kbd>Ctrl</kbd>／<kbd>⌘</kbd> + <kbd>Enter</kbd></dt><dd>在議題輸入框裡開始議題</dd>
      </dl>
      <label class="check"><input type="checkbox" id="scToggle"> 啟用單鍵快捷鍵（? 與 g 系列）</label>
      <p class="muted">在輸入框裡、輸入法組字時不會觸發。這個設定只存在這台裝置。</p>
      <div class="toolbar"><button class="btn small" value="close">關閉</button></div>
    </form>`;
  document.body.appendChild(dialog);
  dialog.querySelector('#scToggle').onchange = (e) => setShortcutsEnabled(e.target.checked);
  dialog.addEventListener('close', () => {
    // Back to whatever opened it; if that element is gone (page re-rendered), the page title.
    const target = opener && document.contains(opener) ? opener : document.querySelector('#view h1');
    target?.focus?.({ preventScroll: true });
    opener = null;
  });
  return dialog;
}

export function openShortcuts() {
  const d = ensureDialog();
  if (d.open) return;
  opener = document.activeElement;
  d.querySelector('#scToggle').checked = shortcutsEnabled();
  d.showModal(); // native modal: background inert, Tab stays inside, Esc closes
}

export function initShortcuts() {
  document.addEventListener('keydown', (e) => {
    if (e.defaultPrevented || e.isComposing || e.ctrlKey || e.metaKey || e.altKey) return;
    if (document.querySelector('dialog[open]')) return;
    if (typingTarget(e.target)) return;
    if (!shortcutsEnabled()) return;
    if (e.key === '?') { e.preventDefault(); openShortcuts(); return; }
    const k = e.key.toLowerCase();
    if (pendingG && Date.now() - pendingG < SEQ_MS && PAGES[k]) {
      e.preventDefault();
      pendingG = 0;
      if (location.hash !== PAGES[k][0]) location.hash = PAGES[k][0];
      return;
    }
    pendingG = k === 'g' && !e.shiftKey ? Date.now() : 0;
  });
  const help = $('#shortcutsBtn');
  if (help) help.onclick = openShortcuts;
}
