// 与 Rust 后端通信；不在 Tauri 中运行（浏览器预览）时回退到 mock.js。
const T = window.__TAURI__;
export const isTauri = !!(T && T.core);

export async function call(cmd, args = {}) {
  if (isTauri) return T.core.invoke(cmd, args);
  return (await import('./mock.js')).mockCall(cmd, args);
}

export async function on(name, cb) {
  if (isTauri) return T.event.listen(name, (e) => cb(e.payload));
  return (await import('./mock.js')).mockOn(name, cb);
}

export async function saveDialog(defaultPath) {
  if (isTauri && T.dialog) {
    return T.dialog.save({ defaultPath, filters: [{ name: 'Markdown', extensions: ['md'] }, { name: '文本', extensions: ['txt'] }] });
  }
  return null;
}

export async function openWav() {
  if (isTauri && T.dialog) {
    return T.dialog.open({ multiple: false, filters: [{ name: 'WAV 音频（16k 单声道 PCM）', extensions: ['wav'] }] });
  }
  return null;
}
