// 提示词模板：名称、整理间隔与一份整体系统提示词。
import { call } from '../api.js';
import { S } from '../state.js';
import { $, $$, esc, icon, toast, confirmBox } from '../ui.js';

let root = null, cur = null;

async function load(selectId) {
  S.templates = await call('templates_list');
  cur = S.templates.find((t) => t.id === selectId) || cur && S.templates.find((t) => t.id === cur.id) || S.templates[0];
  paint();
}

function paint() {
  $('#tpList', root).innerHTML = S.templates.map((t) => `<div class="tpl ${t.id === cur?.id ? 'on' : ''}" data-id="${t.id}"><b>${esc(t.name)}</b><span>每 ${t.interval_min} 分钟${t.builtin ? ' · 预置' : ''}</span></div>`).join('');
  const t = cur;
  $('#tfName', root).value = t.name;
  $('#tfInt', root).value = t.interval_min;
  $('#tfTask', root).value = t.task;
}

export function render(el) {
  root = el;
  root.innerHTML = `<div class="view">
    <div class="tool"><h2 class="h">提示词模板</h2><span class="cap">一份系统提示词，写清会议主题、记录重点和最终输出要求，AI 据此做阶段整理与最终总结</span></div>
    <div class="split">
      <div class="tpl-list"><button class="btn pri" id="tpNew">${icon('plus')}新建模板</button><button class="btn sm ghost" id="tpRestore">恢复预置模板</button><div id="tpList" class="tpl-items"></div></div>
      <div class="form">
        <div class="grid2" style="grid-template-columns:1fr 130px"><div class="fld"><label>模板名称</label><input class="inp" id="tfName" maxlength="30"></div><div class="fld"><label>整理间隔（分钟）</label><input class="inp" id="tfInt" type="number" min="1" max="60"></div></div>
        <div class="fld grow"><label>系统提示词</label><textarea class="inp" id="tfTask" rows="8" placeholder="描述会议主题、要记录的重点，以及最终要输出的内容。例如：你是资深技术面试记录员，请记录候选人的项目经历、技术细节……最终输出：1. 整体总结 2. 分维度打分 3. 待追问清单"></textarea></div>
        <div class="row" style="position:sticky;bottom:0;padding:6px 0;background:linear-gradient(transparent,var(--bg) 40%)"><button class="btn pri" id="tfSave">保存模板</button><button class="btn" id="tfCopy">另存为副本</button><div class="sp"></div><button class="btn ghost" id="tfDel">${icon('trash')}删除</button></div>
      </div></div></div>`;
  $('#tpList', root).onclick = (e) => { const t = e.target.closest('.tpl'); if (!t) return; cur = S.templates.find((x) => x.id === Number(t.dataset.id)); paint(); };
  const save = async (asNew) => {
    try {
      const id = await call('template_save', { id: asNew ? null : cur.id, name: $('#tfName', root).value + (asNew ? '（副本）' : ''), task: $('#tfTask', root).value, outputReq: '', filters: '[]', intervalMin: Number($('#tfInt', root).value) || 5 });
      toast('模板已保存', 'ok'); load(id);
    } catch (e) { toast(String(e), 'error'); }
  };
  $('#tfSave', root).onclick = () => save(false);
  $('#tfCopy', root).onclick = () => save(true);
  $('#tfDel', root).onclick = async () => {
    if (!(await confirmBox('删除模板', `确定删除「${cur.name}」？已有会议不受影响。`))) return;
    try { await call('template_delete', { id: cur.id }); cur = null; load(); toast('已删除', 'ok'); } catch (e) { toast(String(e), 'error'); }
  };
  $('#tpNew', root).onclick = async () => { const id = await call('template_save', { id: null, name: '新模板', task: '', outputReq: '', filters: '[]', intervalMin: 5 }); load(id); };
  $('#tpRestore', root).onclick = async () => { const n = await call('templates_restore'); toast(n ? `已恢复 ${n} 个预置模板` : '预置模板都在', 'ok'); load(cur?.id); };
  load(cur?.id);
}
