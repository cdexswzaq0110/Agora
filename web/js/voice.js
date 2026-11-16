// 語音輸入 — Web Speech API where the browser has it (Chrome/Edge desktop & Android, Safari).
// Needs a secure context (localhost or HTTPS via tailscale serve).

const Recognition = window.SpeechRecognition || window.webkitSpeechRecognition;

export const voiceSupported = () => Boolean(Recognition) && window.isSecureContext;

export function voiceButton(targetId) {
  if (!voiceSupported()) return '';
  return `<button class="btn small quiet voice" type="button" data-voice="${targetId}" aria-label="語音輸入" title="語音輸入（繁體中文）">🎤 語音</button>`;
}

let active = null;
const bound = new WeakSet();

/** Delegate: call once per view root. */
export function bindVoice(root) {
  // #view survives route changes; binding it on every render stacked handlers that started
  // and immediately stopped recognition on one click.
  if (bound.has(root)) return;
  bound.add(root);
  root.addEventListener('click', (e) => {
    const b = e.target.closest('[data-voice]');
    if (!b) return;
    const ta = document.getElementById(b.dataset.voice);
    if (!ta) return;
    if (active) { active.stop(); return; }
    const rec = new Recognition();
    rec.lang = 'zh-TW';
    rec.interimResults = true;
    rec.continuous = false;
    const base = ta.value ? ta.value.replace(/\s*$/, '') + ' ' : '';
    rec.onresult = (ev) => {
      let text = '';
      for (const r of ev.results) text += r[0].transcript;
      ta.value = base + text;
      ta.dispatchEvent(new Event('input', { bubbles: true }));
    };
    rec.onend = () => { active = null; b.classList.remove('on'); b.textContent = '🎤 語音'; };
    rec.onerror = () => rec.onend();
    active = rec;
    b.classList.add('on');
    b.textContent = '● 聆聽中…';
    rec.start();
  });
}
