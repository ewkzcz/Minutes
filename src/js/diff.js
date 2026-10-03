// 字符级差异：用于「纠错对照」，把原始转写与纠错后文本的改动标出来。
import { esc } from './ui.js';

function ops(a, b) {
  let s = 0;
  while (s < a.length && s < b.length && a[s] === b[s]) s++;
  let e = 0;
  while (e < a.length - s && e < b.length - s && a[a.length - 1 - e] === b[b.length - 1 - e]) e++;
  const A = a.slice(s, a.length - e), B = b.slice(s, b.length - e);
  const res = [];
  if (s) res.push(['=', a.slice(0, s)]);
  if (A.length * B.length > 160000 || !A.length || !B.length) {
    if (A) res.push(['-', A]);
    if (B) res.push(['+', B]);
  } else {
    const n = A.length, m = B.length;
    const dp = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
    for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) dp[i][j] = A[i] === B[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    let i = 0, j = 0;
    const push = (t, c) => { const last = res[res.length - 1]; if (last && last[0] === t) last[1] += c; else res.push([t, c]); };
    while (i < n && j < m) {
      if (A[i] === B[j]) { push('=', A[i]); i++; j++; }
      else if (dp[i + 1][j] >= dp[i][j + 1]) push('-', A[i++]);
      else push('+', B[j++]);
    }
    while (i < n) push('-', A[i++]);
    while (j < m) push('+', B[j++]);
  }
  if (e) res.push(['=', a.slice(a.length - e)]);
  return res;
}

/** mode: 'ins' 只标新增（纠错后视图）；'both' 删除线 + 新增（对照视图） */
export function diffHtml(raw, text, mode = 'both') {
  if (raw === text) return esc(text);
  return ops(raw, text)
    .map(([t, c]) => (t === '=' ? esc(c) : t === '+' ? `<ins>${esc(c)}</ins>` : mode === 'both' ? `<del>${esc(c)}</del>` : ''))
    .join('');
}

export const segHtml = (seg, mode) => (mode === 'raw' ? esc(seg.raw) : diffHtml(seg.raw, seg.text, mode === 'diff' ? 'both' : 'ins'));
