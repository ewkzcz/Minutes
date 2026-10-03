// 专有词库：可建多个词库，各自启用 / 停用，并与提示词模板绑定。
// 录制时使用：已启用、且绑定了当前模板或未绑定任何模板（通用）的词库，作为上下文纠错时交给模型的参考依据（不做机械替换）。
import { call } from '../api.js';
import { S } from '../state.js';
import { $, esc, icon, toast, modal, confirmBox, debounce } from '../ui.js';
import { diffHtml } from '../diff.js';

let root = null, cur = null, items = [], q = '';

// 与后端 split_terms 一致：换行、中英文逗号、顿号、中英文分号、制表符分隔，去重（英文忽略大小写）。
const split = (s) => [...new Map(s.split(/[\r\n,，、;；\t]/).map((t) => t.trim()).filter(Boolean).map((t) => [t.toLowerCase(), t])).values()];
const scope = (b) => b.template_ids.length ? `绑定 ${b.template_ids.length} 个模板` : '通用';

async function loadBooks(selectId) {
  [S.books, S.templates] = await Promise.all([call('books_list'), call('templates_list')]);
  cur = S.books.find((b) => b.id === (selectId ?? cur?.id)) || S.books[0] || null;
  paintBooks();
  paintBook();
  await loadTerms();
}

async function loadTerms() { items = cur ? await call('lexicon_list', { bookId: cur.id }) : []; paintTerms(); }

function paintBooks() {
  $('#lbList', root).innerHTML = S.books.map((b) => `<div class="tpl ${b.id === cur?.id ? 'on' : ''}" data-id="${b.id}"><b>${esc(b.name)}</b><span>${b.n} 词 · ${scope(b)}${b.enabled ? '' : ' · 已停用'}</span></div>`).join('');
}

function paintBook() {
  const pane = $('#lbPane', root);
  if (!cur) {
    pane.innerHTML = `<div class="empty">${icon('book')}<b>还没有词库</b><span>按会议类型建立词库，如「后端技术」「客户名单」，再绑定到对应的提示词模板</span><button class="btn pri" data-a="new">${icon('plus')}新建词库</button></div>`;
    return;
  }
  pane.innerHTML = `
    <div class="row" style="gap:12px;align-items:flex-end">
      <div class="fld" style="flex:1"><label>词库名称</label><input class="inp" id="lbName" maxlength="30" value="${esc(cur.name)}"></div>
      <div class="fld" style="align-items:center"><label>启用</label><button class="sw ${cur.enabled ? 'on' : ''}" id="lbOn" aria-label="启用词库" style="margin-bottom:8px"></button></div>
      <button class="btn ghost" id="lbDel" style="margin-bottom:2px">${icon('trash')}删除词库</button>
    </div>
    <div class="fld"><label>绑定提示词模板</label>
      <div class="chips" id="lbTpls">${S.templates.map((t) => `<button class="chip ${cur.template_ids.includes(t.id) ? 'on' : ''}" data-t="${t.id}">${esc(t.name)}</button>`).join('')}</div>
      <span class="cap">${cur.template_ids.length ? '只在使用已绑定模板的会议里生效' : '未绑定任何模板：所有模板的会议都会使用这个词库'}</span></div>
    <div class="row" style="gap:8px"><div class="search" style="flex:1">${icon('search')}<input class="inp" id="lxQ" placeholder="搜索词条" value="${esc(q)}"></div><span class="cap" id="lxCnt"></span><button class="btn pri" id="lxAdd">${icon('plus')}添加词条</button></div>
    <div class="card row" style="padding:8px 10px;gap:8px;flex:none"><input class="inp" id="lxTest" placeholder="试一试：如「我们用卡夫卡做队列」" style="flex:1"><button class="btn" id="lxGo">测试</button></div>
    <div id="lxRes" class="cap" style="min-height:0"></div>
    <div class="card scroll" style="flex:1;min-height:220px"><table class="tbl"><thead><tr><th>专有词</th><th>启用</th><th></th></tr></thead><tbody id="lxBody"></tbody></table></div>`;
  bindBook();
}

function paintTerms() {
  const body = $('#lxBody', root); if (!body) return;
  const list = items.filter((i) => !q || i.term.toLowerCase().includes(q));
  body.innerHTML = list.length ? list.map((i) => `<tr data-id="${i.id}">
      <td>${esc(i.term)}</td>
      <td><button class="sw ${i.enabled ? 'on' : ''}" data-a="tog" aria-label="启用"></button></td>
      <td style="white-space:nowrap"><button class="ib" data-a="edit" title="编辑" aria-label="编辑">${icon('edit')}</button> <button class="ib" data-a="del" title="删除" aria-label="删除">${icon('trash')}</button></td></tr>`).join('')
    : `<tr><td colspan="3"><div class="empty">${icon('book')}<b>${q ? '没有匹配的词条' : '这个词库还是空的'}</b><span>添加会议里常出现的人名、产品名、术语</span></div></td></tr>`;
  $('#lxCnt', root).textContent = `${items.filter((i) => i.enabled).length} / ${items.length} 条启用`;
}

function nameDialog() {
  modal(`<h3>新建词库</h3>
    <div class="fld"><label>词库名称</label><input class="inp" id="fN" maxlength="30" placeholder="如：后端技术、客户名单、团队成员"></div>
    <div class="foot"><button class="btn" data-close>取消</button><button class="btn pri" id="fOk">创建</button></div>`, (m, close) => {
    const ok = async () => {
      try { const id = await call('book_save', { id: null, name: $('#fN', m).value }); close(); q = ''; await loadBooks(id); } catch (e) { toast(String(e), 'error'); }
    };
    $('#fN', m).focus();
    $('#fN', m).onkeydown = (e) => e.key === 'Enter' && ok();
    $('#fOk', m).onclick = ok;
  });
}

function addDialog() {
  modal(`<h3>添加到「${esc(cur.name)}」</h3>
    <div class="fld"><label>专有词</label><textarea class="inp" id="fT" rows="8" placeholder="一行一个，或用逗号、顿号、分号分隔，如：&#10;Kafka、灰度发布、张三丰&#10;Apache Flink"></textarea>
    <span class="cap" id="fN">纠错时这些词会和最近的对话一起交给模型，由模型结合读音与上下文判断是否改成词表写法</span></div>
    <div class="foot"><button class="btn" data-close>取消</button><button class="btn pri" id="fOk">添加</button></div>`, (m, close) => {
    const ta = $('#fT', m), have = new Set(items.map((i) => i.term.toLowerCase()));
    ta.focus();
    ta.oninput = debounce(() => {
      const t = split(ta.value), dup = t.filter((x) => have.has(x.toLowerCase())).length;
      $('#fN', m).textContent = t.length ? `识别到 ${t.length} 个词${dup ? `，其中 ${dup} 个已在本词库将跳过` : ''}` : '一行一个，或用逗号、顿号、分号分隔';
    }, 120);
    $('#fOk', m).onclick = async () => {
      if (!split(ta.value).length) return toast('请输入专有词', 'error');
      try {
        const [added, skipped] = await call('lexicon_add', { bookId: cur.id, text: ta.value });
        close(); loadBooks(); toast(`已添加 ${added} 个${skipped ? `，跳过 ${skipped} 个已存在` : ''}`, 'ok');
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
      try { await call('lexicon_rename', { id: it.id, term: $('#fT', m).value }); close(); loadTerms(); toast('已保存', 'ok'); } catch (e) { toast(String(e), 'error'); }
    };
  });
}

function bindBook() {
  const name = $('#lbName', root);
  const rename = async () => {
    if (name.value.trim() === cur.name) return;
    try { await call('book_save', { id: cur.id, name: name.value }); loadBooks(); } catch (e) { toast(String(e), 'error'); name.value = cur.name; }
  };
  name.onblur = rename;
  name.onkeydown = (e) => e.key === 'Enter' && name.blur();
  $('#lbOn', root).onclick = async () => { await call('book_toggle', { id: cur.id, enabled: !cur.enabled }); loadBooks(); };
  $('#lbDel', root).onclick = async () => {
    if (!(await confirmBox('删除词库', `确定删除「${cur.name}」及其中 ${cur.n} 个词条？绑定关系一并解除。`))) return;
    await call('book_delete', { id: cur.id }); cur = null; loadBooks(); toast('已删除', 'ok');
  };
  $('#lbTpls', root).onclick = async (e) => {
    const c = e.target.closest('[data-t]'); if (!c) return;
    const tid = Number(c.dataset.t);
    await call('book_bind', { bookId: cur.id, templateId: tid, bound: !cur.template_ids.includes(tid) });
    loadBooks();
  };
  $('#lxAdd', root).onclick = addDialog;
  $('#lxQ', root).oninput = debounce((e) => { q = e.target.value.trim().toLowerCase(); paintTerms(); }, 150);
  const test = async () => {
    const text = $('#lxTest', root).value.trim(); if (!text) return;
    const btn = $('#lxGo', root); btn.disabled = true; $('#lxRes', root).textContent = '纠错模型处理中…';
    try {
      const out = await call('lexicon_test', { bookId: cur.id, text });
      $('#lxRes', root).innerHTML = out === text ? '模型认为无需修改，文本保持不变' : `<span style="font-size:13px;color:var(--t1)">${diffHtml(text, out, 'both')}</span>`;
    } catch (e) { $('#lxRes', root).textContent = String(e); } finally { btn.disabled = false; }
  };
  $('#lxGo', root).onclick = test;
  $('#lxTest', root).onkeydown = (e) => e.key === 'Enter' && test();
  $('#lxBody', root).onclick = async (e) => {
    const tr = e.target.closest('tr[data-id]'); if (!tr) return;
    const it = items.find((x) => x.id === Number(tr.dataset.id)); const a = e.target.closest('[data-a]')?.dataset.a;
    if (a === 'tog') { await call('lexicon_toggle', { id: it.id, enabled: !it.enabled }); loadTerms(); }
    else if (a === 'edit') editDialog(it);
    else if (a === 'del' && (await confirmBox('删除词条', `确定删除「${it.term}」？`))) { await call('lexicon_delete', { ids: [it.id] }); loadBooks(); }
  };
}

export function render(el) {
  root = el; q = '';
  root.innerHTML = `<div class="view">
    <div class="tool"><h2 class="h">专有词库</h2><span class="cap">按会议类型分库，绑定到提示词模板；录制时只用已启用、且属于当前模板或通用的词库</span></div>
    <div class="split">
      <div class="tpl-list"><button class="btn pri" data-a="new">${icon('plus')}新建词库</button><div id="lbList" class="tpl-items"></div></div>
      <div class="form" id="lbPane"></div></div></div>`;
  root.onclick = (e) => { if (e.target.closest('[data-a="new"]')) nameDialog(); };
  $('#lbList', root).onclick = (e) => {
    const t = e.target.closest('.tpl'); if (!t) return;
    cur = S.books.find((b) => b.id === Number(t.dataset.id)); q = '';
    paintBooks(); paintBook(); loadTerms();
  };
  loadBooks();
}
