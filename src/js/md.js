// 极简 Markdown 渲染（标题 / 加粗 / 斜体 / 行内代码 / 列表 / 表格 / 分隔线），先转义再渲染。
import { esc } from './ui.js';

const inline = (s) => esc(s)
  .replace(/`([^`]+)`/g, '<code>$1</code>')
  .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
  .replace(/(^|[^*])\*([^*\s][^*]*)\*/g, '$1<em>$2</em>');

export function md(src, mark = '') {
  const lines = String(src || '').replace(/\r/g, '').split('\n');
  const out = [];
  let list = null, para = [];
  const flushP = () => { if (para.length) { out.push(`<p>${inline(para.join(' '))}</p>`); para = []; } };
  const flushL = () => { if (list) { out.push(`</${list}>`); list = null; } };
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    let m;
    if (!l.trim()) { flushP(); flushL(); continue; }
    if ((m = l.match(/^(#{1,4})\s+(.*)$/))) { flushP(); flushL(); out.push(`<h${m[1].length}>${inline(m[2])}</h${m[1].length}>`); continue; }
    if (/^\s*(-{3,}|\*{3,})\s*$/.test(l)) { flushP(); flushL(); out.push('<hr>'); continue; }
    if (l.trim().startsWith('|') && /^\s*\|?[\s:|-]+\|[\s:|-]*$/.test(lines[i + 1] || '')) {
      flushP(); flushL();
      const cells = (r) => r.trim().replace(/^\||\|$/g, '').split('|').map((c) => inline(c.trim()));
      let h = `<table><thead><tr>${cells(l).map((c) => `<th>${c}</th>`).join('')}</tr></thead><tbody>`;
      i += 2;
      for (; i < lines.length && lines[i].trim().startsWith('|'); i++) h += `<tr>${cells(lines[i]).map((c) => `<td>${c}</td>`).join('')}</tr>`;
      i--;
      out.push(h + '</tbody></table>');
      continue;
    }
    if ((m = l.match(/^\s*([-*•])\s+(.*)$/))) { flushP(); if (list !== 'ul') { flushL(); out.push('<ul>'); list = 'ul'; } out.push(`<li>${inline(m[2])}</li>`); continue; }
    if ((m = l.match(/^\s*\d+[.、)]\s+(.*)$/))) { flushP(); if (list !== 'ol') { flushL(); out.push('<ol>'); list = 'ol'; } out.push(`<li>${inline(m[1])}</li>`); continue; }
    flushL();
    para.push(l.trim());
  }
  flushP(); flushL();
  let html = out.join('');
  if (mark) {
    const re = new RegExp(`(${mark.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')})(?![^<]*>)`, 'gi');
    html = html.replace(re, '<mark>$1</mark>');
  }
  return html;
}
