// Progressive motion layer. Content is hidden only after JS has enabled motion.
import { $$, store } from './util.js';

const reduce = window.matchMedia('(prefers-reduced-motion: reduce)');
const fine = window.matchMedia('(pointer: fine)');
const tasks = new Set();
let frame = 0;
let last = 0;
let observer = null;
let cursor = null;

export const motionSetting = () => store.get('motion', 'auto');
// 自動 follows the system's reduced-motion request; 開／關 are your explicit choice on this device.
export const motionOn = () => motionSetting() === 'on' || (motionSetting() === 'auto' && !reduce.matches);

function tick(now) {
  frame = 0;
  if (document.hidden || !motionOn()) return;
  const dt = Math.min(64, now - (last || now));
  last = now;
  tasks.forEach((task) => task(now, dt));
  if (tasks.size) frame = requestAnimationFrame(tick);
}

export function addFrameTask(task) {
  tasks.add(task);
  if (!frame && !document.hidden && motionOn()) frame = requestAnimationFrame(tick);
  return () => tasks.delete(task);
}

function stopMotion() {
  if (frame) cancelAnimationFrame(frame);
  frame = 0;
  document.getAnimations().forEach((animation) => animation.cancel());
  $$('.reveal-pending, .reveal-in').forEach((el) => el.classList.remove('reveal-pending', 'reveal-in'));
  $$('.page-in, .page-rise, .split-ready').forEach((el) => el.classList.remove('page-in', 'page-rise', 'split-ready'));
  $$('[data-magnetic]').forEach((el) => { el.style.removeProperty('transform'); });
  if (cursor) cursor.hidden = true;
}

function applyPreference() {
  const on = motionOn();
  document.documentElement.classList.toggle('motion-on', on);
  document.documentElement.classList.toggle('motion-off', !on);
  // An explicit 開 also lifts the CSS reduced-motion clamp in app.css.
  document.documentElement.classList.toggle('motion-forced', on && motionSetting() === 'on');
  if (!on) stopMotion();
  else {
    if (cursor) cursor.hidden = !fine.matches;
    if (tasks.size && !frame && !document.hidden) frame = requestAnimationFrame(tick);
  }
  document.dispatchEvent(new CustomEvent('agora:motionchange', { detail: { on, setting: motionSetting() } }));
}

export function setMotionSetting(value) {
  const setting = ['auto', 'on', 'off'].includes(value) ? value : 'auto';
  store.set('motion', setting);
  applyPreference();
}

function splitHeading() {
  const heading = document.querySelector('#view h1.display');
  if (!heading || heading.dataset.split === '1') return;
  const original = heading.textContent;
  const chinese = /[\u3400-\u9fff]/.test(original);
  const pieces = chinese ? Array.from(original) : original.split(/(\s+)/);
  heading.textContent = '';
  heading.setAttribute('aria-label', original);
  pieces.forEach((piece, index) => {
    const mask = document.createElement('span');
    mask.className = 'split-mask';
    const inner = document.createElement('span');
    inner.className = 'split-part';
    inner.style.setProperty('--split-i', index);
    inner.textContent = piece;
    mask.appendChild(inner);
    heading.appendChild(mask);
  });
  heading.dataset.split = '1';
  heading.classList.add('split-ready');
}

function revealElements() {
  if (!observer) observer = new IntersectionObserver((entries) => {
    entries.forEach((entry) => {
      if (!entry.isIntersecting) return;
      entry.target.classList.remove('reveal-pending');
      entry.target.classList.add('reveal-in');
      observer.unobserve(entry.target);
    });
  }, { threshold: 0.08, rootMargin: '0px 0px -5% 0px' });
  $$('[data-reveal], #view .block, #view .session-card, #view .digest-item, #view .device, #view .usage-card').forEach((el) => {
    if (el.dataset.revealed === '1') return;
    el.dataset.revealed = '1';
    el.classList.add('reveal-pending');
    observer.observe(el);
  });
}

function bindMagnetic() {
  $$('[data-magnetic]').forEach((el) => {
    if (el.dataset.magneticBound) return;
    el.dataset.magneticBound = '1';
    el.addEventListener('pointermove', (event) => {
      if (!motionOn() || !fine.matches) return;
      const rect = el.getBoundingClientRect();
      const x = Math.max(-18, Math.min(18, (event.clientX - rect.left - rect.width / 2) * 0.32));
      const y = Math.max(-12, Math.min(12, (event.clientY - rect.top - rect.height / 2) * 0.42));
      el.style.transform = `translate3d(${x}px, ${y}px, 0)`;
    });
    el.addEventListener('pointerleave', () => el.style.removeProperty('transform'));
  });
}

function bindScramble() {
  const glyphs = '01アイウエオカキクケコ';
  $$('[data-scramble]').forEach((el) => {
    if (el.dataset.scrambleBound) return;
    el.dataset.scrambleBound = '1';
    const original = el.textContent;
    el.setAttribute('aria-label', original);
    el.addEventListener('pointerenter', () => {
      if (!motionOn() || !fine.matches) return;
      const started = performance.now();
      let removeTask;
      const decode = (now) => {
        const progress = Math.min(1, (now - started) / 420);
        el.textContent = Array.from(original).map((char, i) => {
          if (/\s/.test(char) || i / original.length < progress) return char;
          return glyphs[Math.floor(Math.random() * glyphs.length)];
        }).join('');
        if (progress >= 1 || !motionOn()) { el.textContent = original; removeTask?.(); }
      };
      removeTask = addFrameTask(decode);
    });
  });
}

export function ping(el) {
  if (!el || !motionOn()) return;
  el.classList.remove('motion-ping');
  void el.offsetWidth;
  el.classList.add('motion-ping');
  el.addEventListener('animationend', () => el.classList.remove('motion-ping'), { once: true });
}

export function pageEnter() {
  const main = document.querySelector('#view');
  if (!main || !motionOn()) return;
  main.classList.remove('page-in', 'page-rise');
  void main.offsetWidth;
  main.classList.add('page-in', 'page-rise');
  splitHeading();
  revealElements();
  bindMagnetic();
  bindScramble();
}

function initCursor() {
  cursor = document.createElement('div');
  cursor.className = 'motion-cursor';
  cursor.hidden = !fine.matches || !motionOn();
  cursor.setAttribute('aria-hidden', 'true');
  document.body.appendChild(cursor);
  let tx = -100, ty = -100, x = tx, y = ty, active = false;
  document.addEventListener('pointermove', (e) => { tx = e.clientX; ty = e.clientY; });
  document.addEventListener('pointerover', (e) => { active = Boolean(e.target.closest('a, button, input, select, textarea, summary, [role="button"]')); });
  addFrameTask(() => {
    x += (tx - x) * 0.18; y += (ty - y) * 0.18;
    cursor.style.transform = `translate3d(${x}px, ${y}px, 0) translate(-50%, -50%) scale(${active ? 1.8 : 1})`;
  });
}

export function initMotion() {
  const noise = document.createElement('div');
  noise.className = 'motion-noise';
  noise.setAttribute('aria-hidden', 'true');
  document.body.appendChild(noise);
  initCursor();
  applyPreference();
  reduce.addEventListener('change', applyPreference);
  fine.addEventListener('change', applyPreference);
  document.addEventListener('visibilitychange', () => {
    last = 0;
    if (document.hidden && frame) { cancelAnimationFrame(frame); frame = 0; }
    else if (motionOn() && tasks.size && !frame) frame = requestAnimationFrame(tick);
  });
}
