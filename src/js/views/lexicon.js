// 专有词库：会议里可能出现的人名、产品名、术语，作为上下文纠错时交给模型的参考依据（不做机械替换）。
import { call } from '../api.js';
import { $, esc, icon, toast, modal, confirmBox, debounce } from '../ui.js';
import { diffHtml } from '../diff.js';

let root = null, items = [], q = '';

// 与后端 split_terms 一致：换行、中英文逗号、顿号、中英文分号、制表符分隔，去重（英文忽略大小写）。
const split = (s) => [...new Map(s.split(/[\r\n,，、;；\t]/).map((t) => t.trim()).filter(Boolean).map((t) => [t.toLowerCase(), t])).values()];

async function load() { items = await call('lexicon_list'); paint(); }

function paint() {
  const list = items.filter((i) => !q || i.term.toLowerCase().includes(q));
  const body = $('#lxBody', root);
  body.innerHTML = list.length ? list.map((i) => `<tr data-id="${i.id}">
      <td>${esc(i.term)}</td>
      <td><button class="sw ${i.enabled ? 'on' : ''}" data-a="tog" aria-label="启用"></button></td>
      <td style="white-space:nowrap"><button class="ib" data-a="edit" title="编辑" aria-label="编辑">${icon('edit')}</button> <button class="ib" data-a="del" title="删除" aria-label="删除">${icon('trash')}</button></td></tr>`).join('')
    : `<tr><td colspan="3"><div class="empty">${icon('book')}<b>${q ? '没有匹配的词条' : '词库还是空的'}</b><span>添加会议里常出现的人名、产品名、术语</span></div></td></tr>`;
  $('#lxCnt', root).textContent = `${items.filter((i) => i.enabled).length} / ${items.length} 条启用`;
}

function addDialog() {
  modal(`<h3>添加专有词</h3>
    <div class="fld"><label>专有词</label><textarea class="inp" id="fT" rows="8" placeholder="一行一个，或用逗号、顿号、分号分隔，如：&#10;Kafka、灰度发布、张三丰&#10;Apache Flink"></textarea>
    <span class="cap" id="fN">纠错时这些词会和最近的对话一起交给模型，由模型结合读音与上下文判断是否改成词表写法</span></div>
    <div class="foot"><button class="btn" data-close>取消</button><button class="btn pri" id="fOk">添加</button></div>`, (m, close) => {
    const ta = $('#fT', m), have = new Set(items.map((i) => i.term.toLowerCase()));
    ta.focus();
    ta.oninput = debounce(() => {
      const t = split(ta.value), dup = t.filter((x) => have.has(x.toLowerCase())).length;
      $('#fN', m).textContent = t.length ? `识别到 ${t.length} 个词${dup ? `，其中 ${dup} 个已存在将跳过` : ''}` : '一行一个，或用逗号、顿号、分号分隔';
    }, 120);
    $('#fOk', m).onclick = async () => {
      if (!split(ta.value).length) return toast('请输入专有词', 'error');
      try {
        const [added, skipped] = await call('lexicon_add', { text: ta.value });
        close(); load(); toast(`已添加 ${added} 个${skipped ? `，跳过 ${skipped} 个已存在` : ''}`, 'ok');
      } catch (e) { toast(String(e), 'error'); }
    };
  });
}

function editDialog(it) {
  modal(`<h3>编辑词条</h3>
    <div class="fld"><label>专有词</label><input class="inp" id="fT" value="${esc(it.term)}"></div>
    <div class="foot"><button class="btn" data-close>取消</button><button class="btn pri" id="fOk">保存</button></div>`, (m, close) => {
    $('#fT', m).focus();
    $('#fOk', m).onclick = async () => {
      try { await call('lexicon_rename', { id: it.id, term: $('#fT', m).value }); close(); load(); toast('已保存', 'ok'); } catch (e) { toast(String(e), 'error'); }
    };
  });
}

export function render(el) {
  root = el; q = '';
  root.innerHTML = `<div class="view">
    <div class="tool"><h2 class="h" style="margin-right:4px">专有词库</h2><span class="cap" id="lxCnt"></span><div class="sp"></div>
      <div class="search" style="max-width:220px">${icon('search')}<input class="inp" id="lxQ" placeholder="搜索词条"></div>
      <button class="btn pri" id="lxAdd">${icon('plus')}添加</button></div>
    <div class="card row" style="padding:8px 10px;gap:8px"><input class="inp" id="lxTest" placeholder="试一试：输入一句带识别错误的话，用纠错模型参照词库纠正（如：我们用卡夫卡做队列）" style="flex:1"><button class="btn" id="lxGo">测试</button></div>
    <div id="lxRes" class="cap" style="min-height:0"></div>
    <div class="card scroll" style="flex:1;min-height:0"><table class="tbl"><thead><tr><th>专有词</th><th>启用</th><th></th></tr></thead><tbody id="lxBody"></tbody></table></div></div>`;
  $('#lxAdd', root).onclick = addDialog;
  $('#lxQ', root).oninput = debounce((e) => { q = e.target.value.trim().toLowerCase(); paint(); }, 150);
  const test = async () => {
    const text = $('#lxTest', root).value.trim(); if (!text) return;
    const btn = $('#lxGo', root); btn.disabled = true; $('#lxRes', root).textContent = '纠错模型处理中…';
    try {
      const out = await call('lexicon_test', { text });
      $('#lxRes', root).innerHTML = out === text ? '模型认为无需修改，文本保持不变' : `<span style="font-size:13px;color:var(--t1)">${diffHtml(text, out, 'both')}</span>`;
    } catch (e) { $('#lxRes', root).textContent = String(e); } finally { btn.disabled = false; }
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
