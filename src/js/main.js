import { call } from './api.js';
import { S, isLive } from './state.js';
import { $, icon } from './ui.js';
import { applyTheme } from './theme.js';
import * as live from './views/live.js';
import * as history from './views/history.js';
import * as lexicon from './views/lexicon.js';
import * as templates from './views/templates.js';
import * as settings from './views/settings.js';

const VIEWS = { live, history, lexicon, templates, settings };
const NAV = [['live', '实时记录', 'mic'], ['history', '历史会议', 'list'], ['lexicon', '专有词库', 'book'], ['templates', '提示词模板', 'doc'], ['settings', '设置', 'gear']];

function renderNav() {
  $('#side').innerHTML = `<div class="brand"><i>${icon('mic')}</i><b>Minutes</b></div>` + NAV.map(([k, n, ic]) => `<button class="nav ${S.view === k ? 'on' : ''}" data-v="${k}">${icon(ic)}${n}${k === 'live' && isLive() ? '<span class="dot"></span>' : ''}${k === 'history' && S.sessionCount ? `<small>${S.sessionCount}</small>` : ''}</button>`).join('') +
    `<div class="stat"><div><span>语音识别</span><b>${S.cfg.asr_model.replace('qwen-audio-', '').replace('-asr-flash-streaming', '')}</b></div><div><span>总结</span><b>${S.cfg.summary_model}</b></div><div><span>纠错</span><b>${S.cfg.correct_enabled ? '开' : '关'}</b></div></div>`;
}

function go(view, arg = null) {
  S.view = view; S.arg = arg;
  renderNav();
  const main = $('#main');
  main.innerHTML = '';
  const el = document.createElement('div');
  el.style.cssText = 'display:flex;flex-direction:column;flex:1;min-height:0';
  main.appendChild(el);
  VIEWS[view].render(el, arg);
}

async function boot() {
  if (/Mac/.test(navigator.platform)) document.body.classList.add('mac');
  const info = await call('get_settings');
  S.cfg = info.settings;
  S.asrModels = info.asr_models;
  S.llmModels = info.llm_models;
  S.devices = info.devices;
  [S.templates, S.books] = await Promise.all([call('templates_list'), call('books_list')]);
  S.sessionCount = (await call('sessions_list', { query: '', onlyFav: false })).length;
  S.go = go;
  S.refreshNav = async () => { S.sessionCount = (await call('sessions_list', { query: '', onlyFav: false })).length; renderNav(); };
  applyTheme(S.cfg.theme);
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => applyTheme(S.cfg.theme));
  $('#themeBtn').onclick = async () => {
    const dark = document.documentElement.dataset.theme === 'dark' || (!document.documentElement.dataset.theme && matchMedia('(prefers-color-scheme: dark)').matches);
    S.cfg.theme = dark ? 'light' : 'dark';
    applyTheme(S.cfg.theme);
    call('save_settings', { values: { theme: S.cfg.theme } });
  };
  $('#side').onclick = (e) => { const v = e.target.closest('.nav')?.dataset.v; if (v) go(v); };
  live.initLiveEvents();
  if (info.active_session) { /* 应用重载时录制仍在进行：回到实时页 */ S.live.state = 'recording'; S.live.sid = info.active_session; S.live.t0 = Date.now(); }
  go('live');
}
boot();
