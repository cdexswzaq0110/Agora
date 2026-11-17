// v0.6 review contract, findings and audit trail. All server values are escaped here.
import { esc, stamp } from './util.js';

const VERDICT = {
  pass: ['通過', '✓'], fail: ['未通過', '×'], cannot_verify: ['無法確認', '?'],
};
const OUTCOME = {
  accepted: '採用', rejected: '不採用', version_changed: '審查期間有變動', failed: '呼叫失敗',
};
const STAGE = { spec: '規格', quality: '品質' };
const SEVERITY = { high: '高', medium: '中', low: '低' };

export function skillTagsHtml(skills = []) {
  if (!skills.length) return '';
  return `<span class="skill-tags" aria-label="本次使用的方法">${skills.map((s) => `<span class="skill-tag">${esc(s.id)}@${esc(s.version)}${s.truncated ? '<b>已截斷</b>' : ''}${s.references_omitted ? '<b>省略參考資料</b>' : ''}</span>`).join('')}</span>`;
}

function evidenceHtml(v) {
  if (v.reason) return `<span class="review-reason">原因：${esc(v.reason)}</span>`;
  if (v.evidence_kind === 'code') return `<span>證據：<code>${esc(v.path || v.evidence || '未提供路徑')}</code></span>`;
  if (v.evidence_kind === 'command') return `<span>證據：驗證第 ${esc(v.verify_round ?? '—')} 輪${v.evidence ? ` · ${esc(v.evidence)}` : ''}</span>`;
  if (v.evidence_kind === 'doc') return `<span>證據：${esc(v.evidence || '未提供說明')}</span>`;
  return v.evidence ? `<span>證據：${esc(v.evidence)}</span>` : '<span class="muted">沒有證據</span>';
}

function contractHtml(contract, reviews) {
  if (!contract) return '';
  const review = [...reviews].reverse().find((x) => x.stage === 'spec' && x.outcome === 'accepted' && x.contract_sha256 === contract.sha256);
  const verdicts = new Map((review?.verdicts || []).map((v) => [v.id, v]));
  return `<section class="review-panel" aria-labelledby="contractTitle"><div class="block-head"><span class="index">Acceptance</span><h2 id="contractTitle">驗收合約</h2><span class="aside"><code>${esc((contract.sha256 || '').slice(0, 12))}</code> · ${contract.source === 'markdown' ? 'Markdown' : 'JSON'}</span></div>
    <div class="acceptance-list">${(contract.criteria || []).map((c) => {
      const v = verdicts.get(c.id) || { verdict: 'cannot_verify', reason: '尚無被採用的規格審查判定' };
      const [label, shape] = VERDICT[v.verdict] || VERDICT.cannot_verify;
      return `<article class="acceptance-row result-${esc(v.verdict)}"><span class="result-shape" aria-hidden="true">${shape}</span><div><h3><code>${esc(c.id)}</code> ${esc(c.text)}</h3><p><b>${label}</b> · ${evidenceHtml(v)}</p></div></article>`;
    }).join('')}</div></section>`;
}

function findingsHtml(reviews) {
  const findings = reviews.filter((r) => r.stage === 'quality' && r.outcome === 'accepted').flatMap((r) => r.findings || []);
  if (!findings.length) return '';
  return `<section class="review-panel" aria-labelledby="findingsTitle"><div class="block-head"><span class="index">Quality</span><h2 id="findingsTitle">品質審查發現</h2></div><ul class="finding-list">${findings.map((f) => `<li><span class="severity severity-${esc(f.severity)}">${esc(SEVERITY[f.severity] || f.severity)}</span><code>${esc(f.id)}</code><span>${esc(f.text)}</span>${f.location ? `<code>${esc(f.location)}</code>` : ''}</li>`).join('')}</ul></section>`;
}

function fingerprint(r) {
  if (!r.fp_verify || !r.fp_before || !r.fp_after) return '版本未知';
  return `指紋：驗證時${r.fp_verify === r.fp_before ? '＝' : '≠'}交給審查者時；審查前${r.fp_before === r.fp_after ? '＝' : '≠'}審查後`;
}

function historyHtml(reviews) {
  if (!reviews.length) return '';
  return `<details class="review-history"><summary>審查紀錄（${reviews.length}）</summary><div class="review-history-list">${reviews.map((r) => `<article><h3>第 ${esc(r.round)} 輪 · ${esc(STAGE[r.stage] || r.stage)}審查 · 第 ${esc(r.attempt)} 次</h3><p><b>${esc(OUTCOME[r.outcome] || r.outcome)}</b>${r.same_family ? ' · <span class="same-family">同一家模型</span>' : ''} · ${esc(fingerprint(r))}</p>${(r.problems || []).length ? `<ul>${r.problems.map((p) => `<li>${esc(p)}</li>`).join('')}</ul>` : ''}<time>${esc(stamp(r.at))}</time></article>`).join('')}</div></details>`;
}

export function reviewHtml(record) {
  const reviews = record.reviews || [];
  return contractHtml(record.plan?.contract, reviews) + findingsHtml(reviews) + historyHtml(reviews);
}
