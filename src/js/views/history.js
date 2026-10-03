// 历史会议：查询、置顶、收藏、单个 / 多选删除、查看详情与导出。
import { call, saveDialog, isTauri } from '../api.js';
import { S } from '../state.js';
import { $, $$, esc, icon, fmtMs, fmtDur, fmtDate, toast, modal, confirmBox, debounce } from '../ui.js';
import { md } from '../md.js';
import { segHtml } from '../diff.js';

let root = null;
const H = { query: '', onlyFav: false, multi: false, sel: new Set(), items: [], tab: 'report' };

const hl = (text, q) => {
  const t = esc(text || '');
  if (!q) return t;
  return t.replace(new RegExp(`(${esc(q).replace(/[.*+?^${}()|[\]\\]/g, '\\$&')})`, 'gi'), '<mark>$1</mark>');
};

// ------------------------------------------------------------------ 列表

async function load() {
  H.items = await call('sessions_list', { query: H.query, onlyFav: H.onlyFav });
  S.sessionCount = H.items.length;
  paintList();
}

function renderList() {
  root.innerHTML = `<div class="view">
    <div class="tool">
      <h2 class="h" style="margin-right:4px">历史会议</h2>
      <div class="search">${icon('search')}<input class="inp" id="hQ" placeholder="搜索标题、转写内容、总结…" value="${esc(H.query)}"></div>
      <div class="seg" id="hFav"><button data-f="0" class="${H.onlyFav ? '' : 'on'}">全部</button><button data-f="1" class="${H.onlyFav ? 'on' : ''}">收藏</button></div>
      <button class="btn" id="hMulti">${H.multi ? '完成' : '多选'}</button>
    </div>
    <div class="tool" id="hBulk" ${H.multi ? '' : 'hidden'}>
      <button class="btn sm" id="hAll">全选</button><span class="cap" id="hCnt"></span><div class="sp"></div>
      <button class="btn sm bad" id="hDel">${icon('trash')}删除所选</button>
    </div>
    <div class="list" id="hList"></div></div>`;
  $('#hQ', root).oninput = debounce((e) => { H.query = e.target.value.trim(); load(); }, 220);
  $('#hFav', root).onclick = (e) => { const f = e.target.dataset.f; if (f == null) return; H.onlyFav = f === '1'; $$('#hFav button', root).forEach((b) => b.classList.toggle('on', b.dataset.f === f)); load(); };
  $('#hMulti', root).onclick = () => { H.multi = !H.multi; H.sel.clear(); renderList(); paintList(); };
  $('#hAll', root).onclick = () => { const all = H.items.filter((i) => !isActive(i)); H.sel = H.sel.size === all.length ? new Set() : new Set(all.map((i) => i.id)); paintList(); };
  $('#hDel', root).onclick = () => delMany([...H.sel]);
  $('#hList', root).onclick = onListClick;
  load();
}

const isActive = (it) => it.status === 'recording' || it.status === 'finalizing';

function paintList() {
  const el = $('#hList', root); if (!el) return;
  if (!H.items.length) {
    el.innerHTML = `<div class="empty">${icon('list')}<b>${H.query ? '没有匹配的会议' : H.onlyFav ? '还没有收藏的会议' : '还没有会议记录'}</b><span>${H.query ? '换个关键词试试' : '结束一场会议后会自动保存在这里'}</span></div>`;
  } else {
    el.innerHTML = H.items.map((it) => `<div class="item ${H.sel.has(it.id) ? 'sel' : ''}" data-id="${it.id}">
      ${H.multi ? `<button class="chk ${H.sel.has(it.id) ? 'on' : ''}" data-a="chk" aria-label="选择">${H.sel.has(it.id) ? icon('check') : ''}</button>` : ''}
      <div style="flex:1;min-width:0">
        <div class="ti">${it.pinned ? `<span class="pin-on">${icon('pin', 'fill')}</span>` : ''}<span class="n">${hl(it.title, H.query)}</span>${it.favorite ? `<span class="fav-on">${icon('star', 'fill')}</span>` : ''}${isActive(it) ? '<span class="chip bad">进行中</span>' : ''}</div>
        <div class="meta"><span>${fmtDate(it.started_at)}</span><span>${it.duration_ms ? fmtDur(it.duration_ms) : '—'}</span><span>${esc(it.template_name)}</span><span>${it.seg_count} 句</span></div>
        ${H.query && it.hit ? `<div class="hit">…${hl(it.hit, H.query)}</div>` : ''}
      </div>
      ${H.multi ? '' : `<div class="acts">
        <button class="ib ${it.pinned ? 'pin-on' : ''}" data-a="pin" title="${it.pinned ? '取消置顶' : '置顶'}" aria-label="置顶">${icon('pin')}</button>
        <button class="ib ${it.favorite ? 'fav-on' : ''}" data-a="fav" title="${it.favorite ? '取消收藏' : '收藏'}" aria-label="收藏">${icon('star')}</button>
        <button class="ib" data-a="del" title="删除" aria-label="删除">${icon('trash')}</button></div>`}
    </div>`).join('');
  }
  const c = $('#hCnt', root); if (c) c.textContent = `已选 ${H.sel.size} / ${H.items.length}`;
  const d = $('#hDel', root); if (d) d.disabled = !H.sel.size;
}

async function onListClick(e) {
  const item = e.target.closest('.item'); if (!item) return;
  const id = Number(item.dataset.id), it = H.items.find((x) => x.id === id);
  const act = e.target.closest('[data-a]')?.dataset.a;
  if (H.multi) {
    if (isActive(it)) return toast('进行中的会议不能删除', 'warn');
    H.sel.has(id) ? H.sel.delete(id) : H.sel.add(id);
    return paintList();
  }
  if (act === 'pin') { await call('session_update', { id, pinned: !it.pinned }); return load(); }
  if (act === 'fav') { await call('session_update', { id, favorite: !it.favorite }); return load(); }
  if (act === 'del') return delMany([id]);
  S.go('history', id);
}

async function delMany(ids) {
  if (!ids.length) return;
  if (!(await confirmBox(ids.length > 1 ? `删除 ${ids.length} 场会议` : '删除会议', '将同时删除其转写、阶段纪要与总结，且无法恢复。', '删除'))) return;
  const n = await call('sessions_delete', { ids });
  H.sel.clear();
  toast(`已删除 ${n} 场会议`, 'ok');
  S.refreshNav();
  if (root.querySelector('#hList')) load(); else S.go('history');
}

// ------------------------------------------------------------------ 详情

async function renderDetail(id) {
  root.innerHTML = '<div class="view"><div class="empty"><span class="spin"></span></div></div>';
  let d;
  try { d = await call('session_get', { id }); } catch (e) { toast(String(e), 'error'); return S.go('history'); }
  const s = d.session;
  const fixed = d.segments.filter((g) => g.raw !== g.text).length;
  const tabs = [['report', '总结'], ['stages', `阶段纪要 ${d.stages.length}`], ['text', '纠错后转写'], ['raw', '原始转写'], ['diff', `对照 ${fixed}`]];
  root.innerHTML = `<div class="view">
    <div class="detail-h">
      <button class="ib" id="dBack" title="返回" aria-label="返回">${icon('back')}</button>
      <input class="title" id="dTitle" value="${esc(s.title)}" maxlength="60" aria-label="会议标题">
      <button class="ib ${s.pinned ? 'pin-on' : ''}" id="dPin" title="置顶">${icon('pin')}</button>
      <button class="ib ${s.favorite ? 'fav-on' : ''}" id="dFav" title="收藏">${icon('star')}</button>
      <button class="btn" id="dExp">${icon('download')}导出</button>
      <button class="ib" id="dDel" title="删除">${icon('trash')}</button>
    </div>
    <div class="row"><span class="cap">${fmtDate(s.started_at)} · ${s.duration_ms ? fmtDur(s.duration_ms) : '—'} · ${esc(s.template_name)} · ${d.segments.length} 句</span><div class="sp"></div>
      <div class="seg" id="dTabs">${tabs.map(([k, n]) => `<button data-t="${k}" class="${H.tab === k ? 'on' : ''}">${n}</button>`).join('')}</div></div>
    <div class="card pane sel-ok" id="dPane"></div></div>`;
  $('#dBack', root).onclick = () => S.go('history');
  $('#dTitle', root).onchange = (e) => call('session_update', { id, title: e.target.value.trim() || s.title }).then(() => toast('标题已更新', 'ok'));
  $('#dPin', root).onclick = async () => { await call('session_update', { id, pinned: !s.pinned }); renderDetail(id); };
  $('#dFav', root).onclick = async () => { await call('session_update', { id, favorite: !s.favorite }); renderDetail(id); };
  $('#dDel', root).onclick = () => delMany([id]);
  $('#dExp', root).onclick = () => exportDialog(id, s.title);
  $('#dTabs', root).onclick = (e) => { const t = e.target.dataset.t; if (!t) return; H.tab = t; $$('#dTabs button', root).forEach((b) => b.classList.toggle('on', b.dataset.t === t)); paint(); };

  function paint() {
    const p = $('#dPane', root), tab = H.tab;
    if (tab === 'report') {
      p.innerHTML = s.report
        ? `<div class="md">${md(s.report)}</div>`
        : `<div class="empty"><b>${d.segments.length ? '还没有总结' : '没有转写内容'}</b>${d.segments.length ? `<div class="row"><select class="inp" id="rgModel" style="width:170px">${S.llmModels.map((m) => `<option ${m === S.cfg.summary_model ? 'selected' : ''}>${esc(m)}</option>`).join('')}</select><button class="btn pri" id="rgBtn">${icon('spark')}生成总结</button></div>` : ''}</div>`;
      if (s.report) p.insertAdjacentHTML('beforeend', `<div class="row" style="margin-top:12px"><select class="inp" id="rgModel" style="width:170px;height:28px;padding:2px 24px 2px 8px;font-size:12px">${S.llmModels.map((m) => `<option ${m === (s.llm_model || S.cfg.summary_model) ? 'selected' : ''}>${esc(m)}</option>`).join('')}</select><button class="btn sm" id="rgBtn">${icon('refresh')}换模型重新生成</button></div>`);
      const b = $('#rgBtn', p);
      if (b) b.onclick = async () => {
        b.disabled = true; b.innerHTML = '<span class="spin"></span>生成中…';
        try { await call('regenerate_report', { id, model: $('#rgModel', p).value }); toast('总结已更新', 'ok'); renderDetail(id); } catch (e) { toast(String(e), 'error'); b.disabled = false; b.textContent = '重试'; }
      };
    } else if (tab === 'stages') {
      p.innerHTML = d.stages.length ? d.stages.map((st) => `<div style="margin-bottom:14px"><div class="row"><span class="tag">第 ${st.idx} 段</span><span class="h3">${fmtMs(st.from_ms)} – ${fmtMs(st.to_ms)}</span><span class="cap">${esc(st.model)}</span></div><div class="md">${md(st.content)}</div></div>`).join('') : '<div class="empty"><b>没有阶段纪要</b><span>会议时长不足一个整理周期，或整理失败</span></div>';
    } else {
      if (!d.segments.length) return (p.innerHTML = '<div class="empty"><b>没有转写内容</b></div>');
      const mode = tab === 'text' ? 'text' : tab === 'raw' ? 'raw' : 'diff';
      const segs = tab === 'diff' ? d.segments.filter((g) => g.raw !== g.text) : d.segments;
      p.innerHTML = segs.length ? segs.map((g) => `<div class="ln"><div class="tm">${fmtMs(g.begin_ms)}</div><div class="tx">${segHtml(g, mode)}</div></div>`).join('') : '<div class="empty"><b>没有需要纠错的地方</b></div>';
    }
  }
  paint();
}

function exportDialog(id, title) {
  const o = { report: true, stages: true, text: true, raw: true, diff: false, fmt: 'md' };
  const rows = [['report', '整体总结'], ['stages', '阶段纪要'], ['text', '纠错后转写'], ['raw', 'ASR 原始转写'], ['diff', '纠错对照（标注每处修改）']];
  modal(`<h3>导出「${esc(title)}」</h3>
    ${rows.map(([k, n]) => `<div class="kv"><span>${n}</span><button class="sw ${o[k] ? 'on' : ''}" data-k="${k}" aria-label="${n}"></button></div>`).join('')}
    <div class="kv"><span>格式</span><div class="seg" id="xFmt"><button data-f="md" class="on">Markdown</button><button data-f="txt">纯文本</button></div></div>
    <div class="foot"><button class="btn" id="xCopy">${icon('copy')}复制</button><button class="btn" data-close>取消</button><button class="btn pri" id="xSave">${icon('download')}导出</button></div>`, (m, close) => {
    m.onclick = (e) => {
      const k = e.target.closest('.sw')?.dataset.k; if (k) { o[k] = !o[k]; e.target.closest('.sw').classList.toggle('on', o[k]); }
      const f = e.target.dataset.f; if (f) { o.fmt = f; $$('#xFmt button', m).forEach((b) => b.classList.toggle('on', b.dataset.f === f)); }
    };
    const opts = () => ({ report: o.report, stages: o.stages, text: o.text, raw: o.raw, diff: o.diff, plain: o.fmt === 'txt' });
    $('#xCopy', m).onclick = async () => { await navigator.clipboard.writeText(await call('session_markdown', { id, opts: opts() })); toast('已复制到剪贴板', 'ok'); };
    $('#xSave', m).onclick = async () => {
      const name = title.replace(/[\\/:*?"<>|]/g, '_') + '.' + (o.fmt === 'md' ? 'md' : 'txt');
      if (isTauri) {
        const path = await saveDialog(name); if (!path) return;
        await call('session_export', { id, opts: opts(), path }); toast('已导出：' + path, 'ok');
      } else {
        const blob = new Blob([await call('session_markdown', { id, opts: opts() })], { type: 'text/plain' });
        const a = document.createElement('a'); a.href = URL.createObjectURL(blob); a.download = name; a.click();
      }
      close();
    };
  });
}

export function render(el, arg) {
  root = el;
  if (arg) renderDetail(arg); else renderList();
}
