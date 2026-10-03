// 设置：百炼密钥与连接、模型切换、实时纠错、麦克风、主题。
import { call } from '../api.js';
import { S } from '../state.js';
import { $, esc, icon, toast } from '../ui.js';
import { applyTheme } from '../theme.js';

let root = null;
const opt = (list, cur) => list.map((m) => `<option ${m === cur ? 'selected' : ''}>${esc(m)}</option>`).join('');
const optKV = (list, cur) => list.map(([v, n]) => `<option value="${v}" ${v === cur ? 'selected' : ''}>${n}</option>`).join('');

export function render(el) {
  root = el;
  const c = S.cfg;
  const secret = c.api_key ? '•'.repeat(Math.min(24, c.api_key.length)) : '';
  root.innerHTML = `<div class="view scroll" style="gap:12px">
    <h2 class="h">设置</h2>
    <div class="card sec"><div class="h3">百炼连接</div>
      <div class="kv"><div class="l"><span>API Key</span><span class="cap">保存在本机数据库；也可写入 .env 的 BAILIAN_API_KEY</span></div><div class="r"><div class="pw"><input class="inp" id="sKey" type="password" data-k="api_key" value="${esc(c.api_key)}" placeholder="sk-..." autocomplete="off"><button class="ib" id="sEye" type="button" title="显示 / 隐藏" aria-label="显示或隐藏密钥">${icon('eye')}</button></div></div></div>
      <div class="kv"><div class="l"><span>业务空间 ID</span><span class="cap">可选；填写后走专属域名</span></div><div class="r"><input class="inp" data-k="workspace_id" value="${esc(c.workspace_id)}" placeholder="llm-xxxx"></div></div>
      <div class="kv"><div class="l"><span>对话接口地址</span><span class="cap">可选；覆盖默认的 OpenAI 兼容地址</span></div><div class="r"><input class="inp" data-k="llm_base_url" value="${esc(c.llm_base_url)}" placeholder="默认自动"></div></div>
      <div class="row"><button class="btn" id="sTest">测试连接</button><span id="sRes" class="cap"></span></div></div>
    <div class="card sec"><div class="h3">模型</div>
      <div class="kv"><span>语音识别（流式）</span><div class="r"><select class="inp" data-k="asr_model">${opt(S.asrModels, c.asr_model)}</select></div></div>
      <div class="kv"><span>识别语言</span><div class="r"><select class="inp" data-k="asr_lang">${optKV([['zh-en', '中英混合'], ['zh', '中文'], ['en', '英文']], c.asr_lang)}</select></div></div>
      <div class="kv"><div class="l"><span>总结模型</span><span class="cap">阶段整理与最终总结</span></div><div class="r"><select class="inp" data-k="summary_model">${opt(S.llmModels, c.summary_model)}</select></div></div>
      <div class="kv"><div class="l"><span>纠错模型</span><span class="cap">选低延迟型号，超时自动放弃</span></div><div class="r"><select class="inp" data-k="correct_model">${opt(S.llmModels, c.correct_model)}</select></div></div>
      <div class="kv"><div class="l"><span>添加其他模型</span><span class="cap">输入百炼支持的对话模型名</span></div><div class="r"><input class="inp" id="sAddM" placeholder="如 qwen-plus-latest"><button class="btn" id="sAddB">添加</button></div></div></div>
    <div class="card sec"><div class="h3">实时纠错</div>
      <div class="kv"><div class="l"><span>启用纠错</span><span class="cap">词库快速纠错 + 上下文纠错</span></div><div class="r"><button class="sw ${c.correct_enabled ? 'on' : ''}" id="sCorr" aria-label="启用纠错"></button></div></div>
      <div class="kv"><span>纠错强度</span><div class="r"><select class="inp" data-k="correct_strength">${optKV([['conservative', '保守：只改明显错误'], ['balanced', '适中'], ['aggressive', '积极：结合上下文改写']], c.correct_strength)}</select></div></div>
      <div class="kv"><div class="l"><span>上下文纠错超时（毫秒）</span><span class="cap">超时保留词库纠错结果</span></div><div class="r"><input class="inp" type="number" min="500" max="10000" step="100" data-k="correct_timeout_ms" value="${c.correct_timeout_ms}"></div></div></div>
    <div class="card sec"><div class="h3">输入与外观</div>
      <div class="kv"><div class="l"><span>音频来源</span><span class="cap">默认同时监听麦克风与耳机 / 扬声器里的系统声音</span></div><div class="r"><select class="inp" data-k="audio_source">${optKV([['both', '麦克风 + 系统声音'], ['mic', '仅麦克风'], ['system', '仅系统声音']], c.audio_source || 'both')}</select></div></div>
      <div class="kv"><span>麦克风</span><div class="r"><select class="inp" data-k="mic_device"><option value="">系统默认</option>${opt(S.devices, c.mic_device)}</select></div></div>
      <div class="kv"><span>主题</span><div class="r"><div class="seg" id="sTheme">${[['system', '跟随系统'], ['light', '浅色'], ['dark', '深色']].map(([v, n]) => `<button data-v="${v}" class="${c.theme === v ? 'on' : ''}">${n}</button>`).join('')}</div></div></div></div>
    <div class="cap" style="padding:0 4px 8px">数据保存在本机 SQLite；语音与文本仅发送到你配置的百炼服务。</div></div>`;

  root.onchange = async (e) => {
    const k = e.target.dataset?.k; if (!k) return;
    const v = e.target.value.trim();
    await call('save_settings', { values: { [k]: v } });
    c[k] = k === 'correct_timeout_ms' ? Number(v) : v;
    toast('已保存', 'ok');
  };
  $('#sEye', root).onclick = (e) => { const i = $('#sKey', root); const show = i.type === 'password'; i.type = show ? 'text' : 'password'; e.currentTarget.innerHTML = icon(show ? 'eyeoff' : 'eye'); };
  $('#sCorr', root).onclick = async (e) => { c.correct_enabled = !c.correct_enabled; e.currentTarget.classList.toggle('on', c.correct_enabled); await call('save_settings', { values: { correct_enabled: c.correct_enabled ? '1' : '0' } }); };
  $('#sTheme', root).onclick = async (e) => { const v = e.target.dataset.v; if (!v) return; c.theme = v; applyTheme(v); await call('save_settings', { values: { theme: v } }); [...e.currentTarget.children].forEach((b) => b.classList.toggle('on', b.dataset.v === v)); };
  $('#sAddB', root).onclick = async () => {
    const m = $('#sAddM', root).value.trim(); if (!m) return;
    if (!S.llmModels.includes(m)) { S.llmModels.push(m); await call('save_settings', { values: { custom_models: JSON.stringify(S.llmModels.slice(5)) } }); }
    toast('已添加，可在上方选择', 'ok'); render(root);
  };
  $('#sTest', root).onclick = async (e) => {
    const b = e.currentTarget; b.disabled = true; $('#sRes', root).innerHTML = '<span class="spin"></span> 正在测试…';
    try {
      const r = await call('test_connection');
      const f = (x, n) => `<span class="chip ${x.ok ? 'ok' : 'bad'}">${n} ${x.ok ? '正常' : '失败'}</span>${x.ok ? '' : `<span class="cap"> ${esc(x.error)}</span>`}`;
      $('#sRes', root).innerHTML = f(r.asr, '语音识别') + ' ' + f(r.llm, '对话模型');
    } catch (err) { $('#sRes', root).innerHTML = `<span class="chip bad">${esc(String(err))}</span>`; }
    b.disabled = false;
  };
}
