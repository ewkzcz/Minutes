// 专有词库：维护专有词与常见误识别写法，供实时纠错使用。
import { call } from '../api.js';
import { $, $$, esc, icon, toast, modal, confirmBox, debounce } from '../ui.js';
import { diffHtml } from '../diff.js';

let root = null, items = [], q = '';
const W = { high: ['高', 'ok'], mid: ['中', 'acc'], low: ['低', ''] };

async function load() { items = await call('lexicon_list'); paint(); }

function paint() {
  const list = items.filter((i) => !q || i.term.toLowerCase().includes(q) || i.misspellings.toLowerCase().includes(q));
  const body = $('#lxBody', root);
  body.innerHTML = list.length ? list.map((i) => `<tr data-id="${i.id}">
      <td>${esc(i.term)}</td>
      <td>${i.misspellings.split(/[\n,，、]/).filter(Boolean).map((m) => `<span class="chip">${esc(m)}</span>`).join(' ') || '<span class="cap">仅按读音匹配</span>'}</td>
      <td><span class="chip ${W[i.weight]?.[1] || ''}">${W[i.weight]?.[0] || '中'}</span></td>
      <td><button class="sw ${i.enabled ? 'on' : ''}" data-a="tog" aria-label="启用"></button></td>
      <td style="white-space:nowrap"><button class="ib" data-a="edit" title="编辑" aria-label="编辑">${icon('edit')}</button> <button class="ib" data-a="del" title="删除" aria-label="删除">${icon('trash')}</button></td></tr>`).join('')
    : `<tr><td colspan="5"><div class="empty">${icon('book')}<b>${q ? '没有匹配的词条' : '词库还是空的'}</b><span>添加会议里常出现的人名、产品名、术语</span></div></td></tr>`;
  $('#lxCnt', root).textContent = `${items.filter((i) => i.enabled).length} / ${items.length} 条启用`;
}

function editDialog(it) {
  modal(`<h3>${it ? '编辑词条' : '添加专有词'}</h3>
    <div class="fld"><label>正确写法</label><input class="inp" id="fT" value="${esc(it?.term || '')}" placeholder="如：Kafka、灰度发布、张三丰"></div>
    <div class="fld"><label>常见误识别写法（可选，换行或逗号分隔）</label><textarea class="inp" id="fM" rows="3" placeholder="如：卡夫卡">${esc(it?.misspellings || '')}</textarea><span class="cap">中文词即使不填误写，也会按拼音相似度自动纠正；英文词自动忽略大小写</span></div>
    <div class="fld"><label>纠错力度</label><div class="seg" id="fW">${['high', 'mid', 'low'].map((k) => `<button data-w="${k}" class="${(it?.weight || 'mid') === k ? 'on' : ''}">${{ high: '积极', mid: '适中', low: '保守' }[k]}</button>`).join('')}</div></div>
    <div class="foot"><button class="btn" data-close>取消</button><button class="btn pri" id="fOk">保存</button></div>`, (m, close) => {
    let w = it?.weight || 'mid';
    $('#fW', m).onclick = (e) => { const k = e.target.dataset.w; if (!k) return; w = k; $$('#fW button', m).forEach((b) => b.classList.toggle('on', b.dataset.w === k)); };
    $('#fT', m).focus();
    $('#fOk', m).onclick = async () => {
      try { await call('lexicon_save', { id: it?.id ?? null, term: $('#fT', m).value, misspellings: $('#fM', m).value, weight: w, note: '' }); close(); load(); toast('已保存', 'ok'); } catch (e) { toast(String(e), 'error'); }
    };
  });
}

export function render(el) {
  root = el; q = '';
  root.innerHTML = `<div class="view">
    <div class="tool"><h2 class="h" style="margin-right:4px">专有词库</h2><span class="cap" id="lxCnt"></span><div class="sp"></div>
      <div class="search" style="max-width:220px">${icon('search')}<input class="inp" id="lxQ" placeholder="搜索词条"></div>
      <button class="btn pri" id="lxAdd">${icon('plus')}添加</button></div>
    <div class="card row" style="padding:8px 10px;gap:8px"><input class="inp" id="lxTest" placeholder="试一试：输入一句带识别错误的话，看看词库怎么纠正（如：我们用卡夫卡做队列）" style="flex:1"><button class="btn" id="lxGo">测试</button></div>
    <div id="lxRes" class="cap" style="min-height:0"></div>
    <div class="card scroll" style="flex:1;min-height:0"><table class="tbl"><thead><tr><th>专有词</th><th>误识别写法</th><th>力度</th><th>启用</th><th></th></tr></thead><tbody id="lxBody"></tbody></table></div></div>`;
  $('#lxAdd', root).onclick = () => editDialog(null);
  $('#lxQ', root).oninput = debounce((e) => { q = e.target.value.trim().toLowerCase(); paint(); }, 150);
  const test = async () => {
    const text = $('#lxTest', root).value.trim(); if (!text) return;
    const out = await call('lexicon_test', { text });
    $('#lxRes', root).innerHTML = out === text ? '未命中任何词条，文本保持不变' : `<span style="font-size:13px;color:var(--t1)">${diffHtml(text, out, 'both')}</span>`;
  };
  $('#lxGo', root).onclick = test;
  $('#lxTest', root).onkeydown = (e) => e.key === 'Enter' && test();
  $('#lxBody', root).onclick = async (e) => {
    const tr = e.target.closest('tr[data-id]'); if (!tr) return;
    const it = items.find((x) => x.id === Number(tr.dataset.id)); const a = e.target.closest('[data-a]')?.dataset.a;
    if (a === 'tog') { await call('lexicon_toggle', { id: it.id, enabled: !it.enabled }); load(); }
    else if (a === 'edit') editDialog(it);
    else if (a === 'del' && (await confirmBox('删除词条', `确定删除「${it.term}」？`))) { await call('lexicon_delete', { ids: [it.id] }); load(); }
  };
  load();
}
