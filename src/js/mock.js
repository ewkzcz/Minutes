// 浏览器预览用的模拟后端（仅在非 Tauri 环境加载）：让界面可以脱离桌面壳调试。
const bus = new EventTarget();
const emit = (n, d) => bus.dispatchEvent(new CustomEvent(n, { detail: d }));
export const mockOn = async (name, cb) => { const f = (e) => cb(e.detail); bus.addEventListener(name, f); return () => bus.removeEventListener(name, f); };

const now = Date.now();
const db = {
  settings: { api_key: 'sk-demo', workspace_id: '', llm_base_url: '', asr_model: 'qwen-audio-3.1-asr-flash-streaming', asr_lang: 'zh-en', summary_model: 'qwen3.8-flash', correct_model: 'qwen3.7-flash-2026-07-15', correct_enabled: true, correct_strength: 'balanced', correct_timeout_ms: 2500, interval_min: 5, mic_device: '', audio_source: 'both', theme: 'system' },
  templates: [
    { id: 1, name: '面试复盘与评分', task: '你是一名资深技术面试记录员……', output_req: '1. 整体总结\n2. 分维度打分\n3. 筛选结果', filters: JSON.stringify([{ cond: '3 年以上后端经验', method: '候选人自述年限 ≥ 3', weight: '必须' }]), interval_min: 5, builtin: 1 },
    { id: 2, name: '项目周会纪要', task: '你是项目周会记录员。', output_req: '1. 概要\n2. 待办', filters: '[]', interval_min: 5, builtin: 1 },
  ],
  lexicon: [{ id: 1, term: 'Kafka', enabled: 1 }, { id: 2, term: '灰度发布', enabled: 1 }],
  sessions: [
    { id: 1, title: '后端工程师 · 二面', started_at: now - 3600e3, duration_ms: 3480e3, status: 'done', pinned: 1, favorite: 1, template_name: '面试复盘与评分', report: '### 1. 整体总结\n候选人 4 年后端经验，主导消息平台迁移至 **Kafka**（日均 2 亿条）。\n\n### 2. 维度打分\n\n| 维度 | 评分 | 证据 |\n|---|---|---|\n| 技术深度 | 4.5 | “分区按订单号哈希” |\n| 系统设计 | 4.0 | “先灰度百分之五” |\n\n### 3. 筛选\n- 3 年以上后端经验：**符合**', asr_model: 'qwen-audio-3.1-asr-flash-streaming', llm_model: 'qwen3.8-flash' },
    { id: 2, title: '产品评审周会', started_at: now - 86400e3, duration_ms: 2100e3, status: 'done', pinned: 0, favorite: 0, template_name: '项目周会纪要', report: '本周评审三个需求，结论见待办。', asr_model: 'qwen-audio-3.1-asr-flash-streaming', llm_model: 'qwen3.8-flash' },
    { id: 3, title: '客户访谈 · 华东区', started_at: now - 3 * 86400e3, duration_ms: 1500e3, status: 'done', pinned: 0, favorite: 1, template_name: '通用会议纪要', report: '', asr_model: 'qwen-audio-3.1-asr-flash-streaming', llm_model: 'qwen3.8-flash' },
  ],
  segments: {
    1: [
      { id: 1, seq: 1, begin_ms: 2392e3, end_ms: 2400e3, raw: '那你们线上的消息队列是怎么选型的？', text: '那你们线上的消息队列是怎么选型的？', stage: 'raw' },
      { id: 2, seq: 2, begin_ms: 2407e3, end_ms: 2430e3, raw: '我们用的是卡夫卡，服务是用锐斯特重写的，部署在K八S上。', text: '我们用的是Kafka，服务是用Rust重写的，部署在K8s上。', stage: 'context' },
    ],
  },
  stages: { 1: [{ id: 1, idx: 1, from_ms: 0, to_ms: 300e3, content: '- **项目背景：** 电商履约系统，团队 8 人\n- **亮点：** 推动监控告警体系', model: 'qwen3.8-flash' }] },
  active: null,
};
let nid = 10;

const demo = [
  ['面试官问，你们线上的消息队列是怎么选型的？', '面试官问，你们线上的消息队列是怎么选型的？'],
  ['我们用的是卡夫卡，主要是吞吐量的考虑，日均消息大概两亿条。', '我们用的是Kafka，主要是吞吐量的考虑，日均消息大概两亿条。'],
  ['服务是用锐斯特重写的，部署在K八S上，延迟从一百二十毫秒降到了三十毫秒。', '服务是用Rust重写的，部署在K8s上，延迟从120毫秒降到了30毫秒。'],
];

function simulate() {
  const a = db.active; let i = 0, t = 0;
  a.timer = setInterval(() => {
    emit('level', { v: Math.random() * 0.8 });
    t++;
    if (t % 14 === 1 && i < demo.length) {
      const [raw, fixed] = demo[i]; let k = 0;
      const p = setInterval(() => { k += 3; emit('asr-partial', { text: raw.slice(0, k) }); if (k >= raw.length) { clearInterval(p); const id = nid++; const seg = { id, seq: i + 1, begin_ms: t * 100, end_ms: t * 100 + 3000, raw, text: raw, stage: 'raw', pending: true }; (db.segments[a.id] ||= []).push({ ...seg, pending: undefined }); emit('asr-partial', { text: '' }); emit('segment', seg); setTimeout(() => { const s = db.segments[a.id].find((x) => x.id === id); s.text = fixed; s.stage = 'context'; emit('segment-corrected', { id, text: fixed, stage: 'context' }); }, 700); i++; } }, 60);
    }
  }, 100);
}

export async function mockCall(cmd, a = {}) {
  await new Promise((r) => setTimeout(r, 15));
  switch (cmd) {
    case 'get_settings': return { settings: db.settings, asr_models: ['qwen-audio-3.1-asr-flash-streaming', 'qwen-audio-3.0-asr-flash-streaming', 'fun-asr-realtime'], llm_models: ['qwen3.8-flash', 'qwen3.7-flash-2026-07-15', 'qwen-plus', 'qwen-max', 'qwen-turbo'], devices: ['MacBook 麦克风', 'USB 会议麦'], active_session: null };
    case 'save_settings': Object.assign(db.settings, a.values); return;
    case 'test_connection': return { asr: { ok: true }, llm: { ok: false, error: '模拟：未连接真实后端' } };
    case 'templates_list': return db.templates;
    case 'template_save': { if (a.id) Object.assign(db.templates.find((t) => t.id === a.id), { name: a.name, task: a.task, output_req: a.outputReq, filters: a.filters, interval_min: a.intervalMin }); else db.templates.push({ id: ++nid, name: a.name, task: a.task, output_req: a.outputReq, filters: a.filters, interval_min: a.intervalMin, builtin: 0 }); return a.id || nid; }
    case 'templates_restore': return 0;
    case 'template_delete': db.templates = db.templates.filter((t) => t.id !== a.id); return;
    case 'lexicon_list': return db.lexicon;
    case 'lexicon_add': { const t = [...new Set(a.text.split(/[\r\n,，、;；\t]/).map((x) => x.trim()).filter(Boolean))]; const fresh = t.filter((x) => !db.lexicon.some((i) => i.term.toLowerCase() === x.toLowerCase())); fresh.forEach((term) => db.lexicon.unshift({ id: ++nid, term, enabled: 1 })); return [fresh.length, t.length - fresh.length]; }
    case 'lexicon_rename': db.lexicon.find((x) => x.id === a.id).term = a.term.trim(); return;
    case 'lexicon_toggle': db.lexicon.find((x) => x.id === a.id).enabled = a.enabled ? 1 : 0; return;
    case 'lexicon_delete': db.lexicon = db.lexicon.filter((x) => !a.ids.includes(x.id)); return;
    case 'lexicon_test': return a.text.replace('卡夫卡', 'Kafka');
    case 'sessions_list': {
      const q = a.query.toLowerCase();
      return db.sessions.filter((s) => (!a.onlyFav || s.favorite) && (!q || s.title.toLowerCase().includes(q) || s.report.toLowerCase().includes(q) || (db.segments[s.id] || []).some((g) => g.text.toLowerCase().includes(q))))
        .sort((x, y) => y.pinned - x.pinned || y.started_at - x.started_at)
        .map((s) => ({ ...s, seg_count: (db.segments[s.id] || []).length, hit: q ? ((db.segments[s.id] || []).find((g) => g.text.toLowerCase().includes(q)) || {}).text : null }));
    }
    case 'session_get': return { session: db.sessions.find((s) => s.id === a.id), segments: db.segments[a.id] || [], stages: db.stages[a.id] || [] };
    case 'session_update': { const s = db.sessions.find((x) => x.id === a.id); if (a.title != null) s.title = a.title; if (a.pinned != null) s.pinned = a.pinned ? 1 : 0; if (a.favorite != null) s.favorite = a.favorite ? 1 : 0; return; }
    case 'sessions_delete': db.sessions = db.sessions.filter((s) => !a.ids.includes(s.id)); return a.ids.length;
    case 'regenerate_report': db.sessions.find((s) => s.id === a.id).report = '### 重新生成的总结\n（模拟）'; return 'ok';
    case 'session_markdown': return '# 导出预览\n\n（模拟内容）';
    case 'start_session': { const id = ++nid; db.sessions.unshift({ id, title: a.title || '会议 演示', started_at: Date.now(), duration_ms: 0, status: 'recording', pinned: 0, favorite: 0, template_name: db.templates[0].name, report: '', asr_model: db.settings.asr_model, llm_model: '' }); db.active = { id }; simulate(); setTimeout(() => emit('session-status', { state: 'recording', session_id: id }), 10); setTimeout(() => { emit('stage-start', {}); setTimeout(() => emit('stage', { id: 99, idx: 1, from_ms: 0, to_ms: 300e3, content: '- **系统设计：** 迁移到 **Kafka**，日均 2 亿条\n- **性能：** 延迟 120ms → 30ms\n- **待追问：** 一致性方案与回滚演练', model: db.settings.summary_model }), 900); }, 6000); return { id, title: a.title || '会议 演示' }; }
    case 'pause_session': emit('session-status', { state: a.paused ? 'paused' : 'recording', session_id: db.active?.id }); return;
    case 'stop_session': { const id = db.active.id; clearInterval(db.active.timer); emit('session-status', { state: 'finalizing', session_id: id }); setTimeout(() => { const s = db.sessions.find((x) => x.id === id); s.status = 'done'; s.duration_ms = 42000; s.report = '### 整体总结\n（模拟总结）'; db.active = null; emit('session-status', { state: 'done', session_id: id }); }, 1200); return; }
    default: return;
  }
}
