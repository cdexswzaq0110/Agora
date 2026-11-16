// A small, safe Markdown renderer: escape first, then format. Handles headings, lists,
// tables, code fences, blockquotes, evidence-grade tags and VERDICT lines.
import { esc } from './util.js';

export const GRADE = {
  confirmed: { zh: '已確認', cls: 'confirmed', tip: '直接觀察或測試過' },
  inferred: { zh: '推論', cls: 'inferred', tip: '由已確認的事實推得，沒有直接測試' },
  candidate: { zh: '候選', cls: 'candidate', tip: '合理，但還有其他可能沒排除' },
  unknown: { zh: '未知', cls: 'unknown', tip: '查過但找不到' },
  unverified: { zh: '未驗證', cls: 'unverified', tip: '宣稱或計畫中，尚未測試' },
};
const GRADE_BY_ZH = { 已確認: 'confirmed', 推論: 'inferred', 候選: 'candidate', 未知: 'unknown', 未驗證: 'unverified' };

export function inline(s) {
  return String(s).split(/(`[^`\n]+`)/g).map((part) => {
    if (/^`[^`\n]+`$/.test(part)) return `<code>${esc(part.slice(1, -1))}</code>`;
    let t = esc(part);
    t = t.replace(/\*\*([^*\n]+)\*\*/g, '<strong>$1</strong>');
    t = t.replace(/(^|[^*\w])\*([^*\s][^*\n]*)\*(?=[^*\w]|$)/g, '$1<em>$2</em>');
    t = t.replace(/\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>');
    t = t.replace(/【(已確認|推論|候選|未知|未驗證)】/g, (m, g) => `<span class="eg ${GRADE[GRADE_BY_ZH[g]].cls}" title="${GRADE[GRADE_BY_ZH[g]].tip}">${g}</span>`);
    t = t.replace(/(^|\s)(✅|❌|⚠️)/g, (m, sp, mark) => `${sp}<span class="mark mark-${mark === '✅' ? 'ok' : mark === '❌' ? 'bad' : 'warn'}">${mark}</span>`);
    return t;
  }).join('');
}

const LIST_RE = /^(\s*)([-*•+]|\d+[.)])\s+(.*)$/;
const TABLE_RE = /^\s*\|.*\|\s*$/;

function table(rows) {
  const cells = (r) => r.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map((c) => c.trim());
  const isSep = (r) => cells(r).every((c) => /^:?-{2,}:?$/.test(c));
  const body = rows.filter((r) => !isSep(r));
  if (!body.length) return '';
  const [head, ...rest] = body;
  const hasHead = rows.length > 1 && isSep(rows[1]);
  const tr = (r, tag) => `<tr>${cells(r).map((c) => `<${tag}>${inline(c)}</${tag}>`).join('')}</tr>`;
  return `<table>${hasHead ? `<thead>${tr(head, 'th')}</thead><tbody>${rest.map((r) => tr(r, 'td')).join('')}</tbody>` : `<tbody>${body.map((r) => tr(r, 'td')).join('')}</tbody>`}</table>`;
}

export function md(src) {
  const lines = String(src || '').replace(/\r\n?/g, '\n').split('\n');
  let html = '';
  const para = [];
  const flush = () => { if (para.length) { html += `<p>${para.map(inline).join('<br>')}</p>`; para.length = 0; } };
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (/^\s*```/.test(line)) {
      flush();
      const buf = [];
      i++;
      while (i < lines.length && !/^\s*```/.test(lines[i])) { buf.push(lines[i]); i++; }
      i++;
      html += `<pre><code>${esc(buf.join('\n'))}</code></pre>`;
      continue;
    }
    if (/^\s*$/.test(line)) { flush(); i++; continue; }
    const hm = line.match(/^\s*(#{1,6})\s+(.*)$/);
    if (hm) { flush(); const l = Math.min(4, hm[1].length); html += `<h${l}>${inline(hm[2])}</h${l}>`; i++; continue; }
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) { flush(); html += '<hr>'; i++; continue; }
    const vm = line.match(/^\s*`?\s*VERDICT\s*[:：]\s*(APPROVE|REVISE)\s*`?\s*$/i);
    if (vm) {
      flush();
      const ok = vm[1].toUpperCase() === 'APPROVE';
      html += `<p><span class="vline ${ok ? 'approve' : 'revise'}">VERDICT · ${ok ? '通過' : '需要修訂'}</span></p>`;
      i++;
      continue;
    }
    if (TABLE_RE.test(line)) {
      flush();
      const rows = [];
      while (i < lines.length && TABLE_RE.test(lines[i])) { rows.push(lines[i]); i++; }
      html += table(rows);
      continue;
    }
    if (/^\s*>/.test(line)) {
      flush();
      const buf = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) { buf.push(lines[i].replace(/^\s*>\s?/, '')); i++; }
      html += `<blockquote>${md(buf.join('\n'))}</blockquote>`;
      continue;
    }
    if (LIST_RE.test(line)) {
      flush();
      const ordered = /\d/.test(line.match(LIST_RE)[2]);
      const items = [];
      while (i < lines.length) {
        const m = lines[i].match(LIST_RE);
        if (m) { items.push(m[3]); i++; continue; }
        if (items.length && /^\s{2,}\S/.test(lines[i])) { items[items.length - 1] += '\n' + lines[i].trim(); i++; continue; }
        break;
      }
      const tag = ordered ? 'ol' : 'ul';
      html += `<${tag}>${items.map((it) => `<li>${it.split('\n').map(inline).join('<br>')}</li>`).join('')}</${tag}>`;
      continue;
    }
    para.push(line);
    i++;
  }
  flush();
  return html;
}

/** Turn `[n]` citations in rendered digest HTML into links to the item cards. */
export function withRefs(html, max) {
  return html.replace(/\[(\d{1,3})\](?![^<]*<\/code>)/g, (m, n) => (Number(n) >= 1 && Number(n) <= max ? `<a class="ref" href="#item-${n}" data-ref="${n}">${n}</a>` : m));
}
