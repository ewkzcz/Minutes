// 实时记录：开始页 + 录制页（实时转写 / 纠错 / 阶段纪要）。
import { call, on, openWav } from '../api.js';
import { S, isLive, elapsed } from '../state.js';
import { $, $$, esc, icon, fmtMs, toast, confirmBox } from '../ui.js';
import { md } from '../md.js';
import { segHtml } from '../diff.js';

let root = null;

const modelOpts = (list, cur) => list.map((m) => `<option ${m === cur ? 'selected' : ''}>${esc(m)}</option>`).join('');
const tplOpts = () => S.templates.map((t) => `<option value="${t.id}" ${String(t.id) === String(S.cfg.templateId || S.templates[0]?.id) ? 'selected' : ''}>${esc(t.name)}</option>`).join('');

// ------------------------------------------------------------------ 开始页

const SOURCES = [['both', 'both', '全部'], ['mic', 'mic', '麦克风'], ['system', 'headphones', '系统声音']];

function renderIdle() {
  const noKey = !S.cfg.api_key;
  const selTpl = S.cfg.templateId ? Number(S.cfg.templateId) : S.templates[0]?.id;
  S.cfg.templateId = selTpl;
  const src = S.cfg.audio_source || 'both';
  root.innerHTML = `<div class="view idle2">
    <div class="i-left">
      <div class="eyebrow">New session</div>
      <h1 class="hero-t">把会议，<br><em>写成笔记。</em></h1>
      <p class="sub">实时转写 · 实时纠错 · 每 ${S.templates.find((t) => t.id === selTpl)?.interval_min || 5} 分钟整理</p>
      ${noKey ? `<div class="chip warn" style="height:auto;padding:9px 16px;white-space:normal;border-radius:20px">未检测到百炼密钥，请先到「设置」填写，或写入 .env</div>` : ''}
      <input class="inp" id="lvTitle" placeholder="给这场会议起个名字（可不填）" maxlength="60">
      <div class="seg" id="lvSrc" style="align-self:flex-start">${SOURCES.map(([k, ic, n]) => `<button data-s="${k}" class="${src === k ? 'on' : ''}">${icon(ic)}${n}</button>`).join('')}</div>
      <div class="row" style="margin-top:4px">
        <button class="go" id="lvStart">${icon('mic')}开始录制</button>
        <button class="btn lg" id="lvFile" title="用 16k 单声道 WAV 文件模拟会议（演示 / 联调）">${icon('folder')}WAV</button>
      </div>
      <div class="cap">自动监听麦克风与耳机里的会议声音</div>
    </div>
    <div class="i-right">
      <div class="row"><div class="eyebrow">提示词模板</div><div class="sp"></div><button class="btn sm" id="lvManage">查看 / 管理</button></div>
      <div class="tcards" id="lvTpls">${S.templates.map((t) => `<button class="tcard ${t.id === selTpl ? 'on' : ''}" data-id="${t.id}"><b>${esc(t.name)}</b><span>${esc((t.task || '').slice(0, 26) || '空白模板')} · ${t.interval_min} 分钟</span></button>`).join('')}</div>
      <div class="grid2"><select class="inp" id="lvAsr" title="语音识别模型" style="padding-left:16px">${modelOpts(S.asrModels, S.cfg.asr_model)}</select><select class="inp" id="lvSum" title="总结模型" style="padding-left:16px">${modelOpts(S.llmModels, S.cfg.summary_model)}</select></div>
    </div></div>`;
  const save = (k) => (e) => call('save_settings', { values: { [k]: e.target.value } }).then(() => (S.cfg[k] = e.target.value));
  $('#lvAsr', root).onchange = save('asr_model');
  $('#lvSum', root).onchange = save('summary_model');
  $('#lvManage', root).onclick = () => S.go('templates');
  $('#lvTpls', root).onclick = (e) => { const c = e.target.closest('.tcard'); if (!c) return; S.cfg.templateId = Number(c.dataset.id); renderIdle(); };
  $('#lvSrc', root).onclick = (e) => { const b = e.target.closest('button'); if (!b) return; S.cfg.audio_source = b.dataset.s; call('save_settings', { values: { audio_source: b.dataset.s } }); $$('#lvSrc button', root).forEach((x) => x.classList.toggle('on', x === b)); };
  $('#lvStart', root).onclick = () => start(null);
  $('#lvFile', root).onclick = async () => {
    const f = await openWav();
    if (f) start(f);
    else toast('请在桌面应用中选择 WAV 文件', 'warn');
  };
}

async function start(audioFile) {
  const l = S.live;
  const tid = Number(S.cfg.templateId || S.templates[0]?.id);
  const title = $('#lvTitle', root)?.value.trim() || '';
  try {
    const r = await call('start_session', { title, templateId: tid, audioFile });
    const tpl = S.templates.find((t) => t.id === tid);
    Object.assign(l, { state: 'recording', sid: r.id, title: r.title, segs: [], stages: [], partial: '', t0: Date.now(), pausedAt: 0, pausedTotal: 0, interval: tpl?.interval_min || 5, summarizing: false, model: S.cfg.summary_model, tab: 'text', levels: new Array(36).fill(0) });
    S.refreshNav();
    render(root);
  } catch (e) {
    toast(String(e), 'error');
  }
}

// ------------------------------------------------------------------ 录制页

const stateChip = () => {
  const s = S.live.state;
  return { paused: '<span class="chip warn">已暂停</span>', reconnecting: '<span class="chip warn">重连中…</span>', finalizing: '<span class="chip acc"><span class="spin"></span>生成总结…</span>' }[s] || '<span class="chip ok">实时纠错' + (S.cfg.correct_enabled ? '已开启' : '已关闭') + '</span>';
};

function renderLive() {
  const l = S.live;
  const bars = '<i></i>'.repeat(36);
  const tab = l.tab || 'text';
  root.innerHTML = `<div class="view" style="gap:16px">
    <div class="row">
      <h2 class="h" style="font-size:28px;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(l.title)}</h2>
      <span id="lvChip">${stateChip()}</span><span class="cap mono" id="lvClock" style="font:700 14px var(--mono);font-variant-numeric:tabular-nums">${fmtMs(elapsed())}</span>
      <div class="sp"></div>
      <select class="inp" id="lvModel" style="width:158px;padding:8px 32px 8px 16px;font-size:13.5px" title="切换后，之后的阶段整理与总结使用该模型">${modelOpts(S.llmModels, l.model)}</select>
    </div>
    <div class="tabs-row">
      <div class="seg" id="lvTab"><button data-t="text" class="${tab === 'text' ? 'on' : ''}">实时转写</button><button data-t="stages" class="${tab === 'stages' ? 'on' : ''}">阶段纪要 <span id="lvBadge">${l.stages.length}</span></button></div>
      <div class="sp"></div>
      <div class="seg" id="lvMode" ${tab === 'text' ? '' : 'hidden'}><button data-m="text" class="${l.mode === 'text' ? 'on' : ''}">纠错后</button><button data-m="diff" class="${l.mode === 'diff' ? 'on' : ''}">对照</button><button data-m="raw" class="${l.mode === 'raw' ? 'on' : ''}">原始</button></div>
    </div>
    <div class="wavebox">${icon('mic')}<div class="wave" id="lvWave">${bars}</div>
      <div class="nextpill"><div class="ring" id="lvRing"><span id="lvRingTxt">--</span></div><div><div class="h3">下次整理</div><div class="cap" id="lvNextCap">每 ${l.interval} 分钟</div></div></div></div>
    <div class="tr" id="lvTr" aria-live="polite" ${tab === 'text' ? '' : 'hidden'}></div>
    <div class="sums" id="lvSums" ${tab === 'stages' ? '' : 'hidden'}></div>
    <div class="card ctl">
      <button class="rbtn" id="lvStop" title="结束并生成总结" aria-label="结束录制"><i></i></button>
      <button class="btn" id="lvPause">${icon(l.state === 'paused' ? 'play' : 'pause')}<span>${l.state === 'paused' ? '继续' : '暂停'}</span></button>
      <button class="btn" id="lvNow">${icon('spark')}立即整理</button>
      <div class="sp"></div><span class="cap" id="lvStat"></span>
    </div></div>`;
  paintSegs();
  paintStages();
  paintWave();
  tick();
  $('#lvTab', root).onclick = (e) => {
    const t = e.target.closest('button')?.dataset.t; if (!t) return;
    l.tab = t;
    $$('#lvTab button', root).forEach((b) => b.classList.toggle('on', b.dataset.t === t));
    $('#lvTr', root).hidden = t !== 'text'; $('#lvSums', root).hidden = t !== 'stages'; $('#lvMode', root).hidden = t !== 'text';
    if (t === 'text') { const tr = $('#lvTr', root); tr.scrollTop = tr.scrollHeight; }
  };
  $('#lvMode', root).onclick = (e) => {
    const m = e.target.dataset.m; if (!m) return;
    l.mode = m; $$('#lvMode button', root).forEach((b) => b.classList.toggle('on', b.dataset.m === m)); paintSegs();
  };
  $('#lvModel', root).onchange = (e) => { l.model = e.target.value; call('set_live_model', { model: l.model }); toast(`后续整理将使用 ${l.model}`); };
  $('#lvPause', root).onclick = () => call('pause_session', { paused: l.state !== 'paused' });
  $('#lvNow', root).onclick = () => { call('summarize_now'); toast('已请求立即整理'); };
  $('#lvStop', root).onclick = async () => { if (await confirmBox('结束会议', '结束后将生成阶段纪要与最终总结，并保存到历史。', '结束并生成总结', false)) call('stop_session'); };
}

function segNode(seg) {
  const d = document.createElement('div');
  d.className = 'ln'; d.dataset.id = seg.id;
  d.innerHTML = `<div class="tm">${fmtMs(seg.begin_ms)}</div><div class="tx"></div>`;
  fillSeg(d, seg);
  return d;
}
function fillSeg(node, seg) {
  node.querySelector('.tx').innerHTML = segHtml(seg, S.live.mode) + (seg.pending ? ' <span class="chip acc" style="height:18px;font-size:10px">纠错中</span>' : '');
}
function paintSegs() {
  const tr = $('#lvTr', root); if (!tr) return;
  const l = S.live;
  tr.innerHTML = '';
  if (!l.segs.length && !l.partial) tr.innerHTML = '<div class="empty"><b>正在聆听…</b><span>开口说话，文字会实时出现在这里</span></div>';
  l.segs.forEach((s) => tr.appendChild(segNode(s)));
  paintPartial();
  tr.scrollTop = tr.scrollHeight;
  paintCount();
}
function paintPartial() {
  const tr = $('#lvTr', root); if (!tr) return;
  let cur = $('.ln.cur', tr);
  const t = S.live.partial;
  if (!t) return cur && cur.remove();
  $('.empty', tr)?.remove();
  if (!cur) { cur = document.createElement('div'); cur.className = 'ln cur'; cur.innerHTML = '<div class="tm">···</div><div class="tx"></div>'; tr.appendChild(cur); }
  cur.querySelector('.tx').textContent = t;
}
function paintCount() {
  const c = $('#lvCount', root), s = $('#lvStat', root); if (!c) return;
  const n = S.live.segs.reduce((a, g) => a + g.text.length, 0), fixed = S.live.segs.filter((g) => g.raw !== g.text).length;
  c.textContent = `${S.live.segs.length} 句`;
  s.textContent = `已识别 ${n.toLocaleString()} 字 · 已纠错 ${fixed} 处`;
}
function paintStages() {
  const el = $('#lvSums', root); if (!el) return;
  const l = S.live;
  const cards = [...l.stages].reverse().map((s, i) => `<div class="card sm"><div class="top"><span class="tag">第 ${s.idx} 段</span><span class="h3">${fmtMs(s.from_ms)} – ${fmtMs(s.to_ms)}</span>${i === 0 ? '<span class="sp"></span><span class="chip ok">最新</span>' : ''}</div><div class="md">${md(s.content)}</div></div>`);
  const busy = l.summarizing ? `<div class="card sm row"><span class="spin"></span><span class="cap">AI 正在整理本段…</span></div>` : '';
  const bg = $('#lvBadge', root); if (bg) bg.textContent = l.stages.length;
  el.innerHTML = busy + (cards.join('') || (l.summarizing ? '' : '<div class="empty"><b>暂无阶段纪要</b><span>每 ' + l.interval + ' 分钟自动整理一次<br>也可点「立即整理」</span></div>'));
}
function paintWave() {
  $$('#lvWave i', root).forEach((b, i) => (b.style.transform = `scaleY(${Math.max(0.12, Math.min(1, S.live.levels[i] || 0))})`));
}

function tick() {
  const l = S.live;
  const t = elapsed();
  const clock = $('#lvClock', root); if (clock) clock.textContent = fmtMs(t);
  const per = l.interval * 60000;
  const remain = per - (t % per);
  const r = $('#lvRingTxt', root);
  if (r) { r.textContent = fmtMs(remain); $('#lvRing', root).style.setProperty('--p', ((t % per) / per) * 100); }
  const rec = $('#tbRec');
  rec.hidden = !isLive();
  rec.className = 'tb-rec' + (l.state === 'paused' ? ' paused' : '');
  rec.textContent = l.state === 'finalizing' ? '生成总结…' : (l.state === 'paused' ? '已暂停 ' : '录制中 ') + fmtMs(t);
}
setInterval(() => { if (isLive()) tick(); else $('#tbRec').hidden = true; }, 1000);

// ------------------------------------------------------------------ 事件

export function initLiveEvents() {
  const l = S.live;
  const mounted = () => root && S.view === 'live' && isLive();
  on('session-status', ({ state, session_id }) => {
    if (state === 'done') {
      const sid = l.sid ?? session_id;
      Object.assign(l, { state: 'idle', sid: null, partial: '' });
      S.refreshNav();
      $('#tbRec').hidden = true;
      toast('会议已保存到历史', 'ok');
      if (S.view === 'live') S.go('history', sid);
      return;
    }
    if (state === 'paused') l.pausedAt = Date.now();
    if (l.state === 'paused' && state === 'recording') l.pausedTotal += Date.now() - l.pausedAt;
    l.state = state;
    if (mounted()) {
      $('#lvChip', root).innerHTML = stateChip();
      const p = $('#lvPause', root); if (p) p.innerHTML = `${icon(state === 'paused' ? 'play' : 'pause')}<span>${state === 'paused' ? '继续' : '暂停'}</span>`;
      if (state === 'finalizing') $$('#lvPause,#lvNow,#lvStop', root).forEach((b) => (b.disabled = true));
    }
    S.refreshNav();
  });
  on('asr-partial', ({ text }) => { l.partial = text; if (mounted()) { const tr = $('#lvTr', root); const near = tr.scrollHeight - tr.scrollTop - tr.clientHeight < 80; paintPartial(); if (near) tr.scrollTop = tr.scrollHeight; } });
  on('segment', (seg) => {
    l.segs.push(seg);
    if (mounted()) { const tr = $('#lvTr', root); const near = tr.scrollHeight - tr.scrollTop - tr.clientHeight < 80; $('.empty', tr)?.remove(); const cur = $('.ln.cur', tr); tr.insertBefore(segNode(seg), cur); if (near) tr.scrollTop = tr.scrollHeight; paintCount(); }
  });
  on('segment-corrected', ({ id, text, stage }) => {
    const seg = l.segs.find((s) => s.id === id); if (!seg) return;
    Object.assign(seg, { text, stage, pending: false });
    if (mounted()) { const n = $(`.ln[data-id="${id}"]`, root); if (n) fillSeg(n, seg); paintCount(); }
  });
  on('stage-start', () => { l.summarizing = true; mounted() && paintStages(); });
  on('stage-fail', () => { l.summarizing = false; mounted() && paintStages(); });
  on('stage', (st) => { l.summarizing = false; l.stages.push(st); mounted() && paintStages(); toast(`第 ${st.idx} 段阶段纪要已生成`, 'ok'); });
  on('level', ({ v }) => { l.levels.push(v); l.levels.shift(); if (mounted()) paintWave(); });
  on('notice', ({ level, text }) => toast(text, level));
  on('report', () => toast('最终总结已生成', 'ok'));
}

export function render(el) {
  root = el;
  if (S.live.state === 'idle') renderIdle(); else renderLive();
}
