import re
css=open('ui.css').read()
def body(f,theme):
    s=open(f).read(); m=re.search(r'<x-dc>(.*)</x-dc>',s,re.S).group(1)
    hs=''.join(re.findall(r'<helmet>\s*<style>(.*?)</style>',m,re.S))
    return hs,re.sub(r'<helmet>.*?</helmet>','',m,flags=re.S).replace('{{theme}}',theme)
pages=[('Main.dc.html','dark'),('Main.dc.html','light'),('Prompt.dc.html','light'),('Prompt.dc.html','dark'),('Export.dc.html','dark'),('Export.dc.html','light'),('Tech.dc.html','light')]
cover='''<section class="app dark" style="page-break-after:always"><div class="aurora"></div><div class="win" style="justify-content:center;padding:0 140px;gap:18px">
<div class="cap" style="font-size:16px;letter-spacing:.12em">UI 设计文档 · v1.0</div>
<h1 style="font-size:88px;letter-spacing:-.035em;line-height:1.05;margin:0;background:linear-gradient(90deg,#64AAFF,#BF5AF2);-webkit-background-clip:text;color:transparent">会议语音纪要</h1>
<div style="font-size:26px;color:var(--t2);line-height:1.6">实时 ASR · 实时纠错 · 每 5 分钟 AI 阶段整理<br>自定义系统提示词 · 原始转写与纠错文本双输出</div>
<div style="display:flex;gap:10px;margin-top:20px"><span class="chip acc">Tauri 桌面应用</span><span class="chip">明 / 暗主题</span><span class="chip">Apple 设计原则</span><span class="chip">安装包本体约 20 MB</span></div></div></section>'''
extra='';out=''
for f,t in pages:
    hs,m=body(f,t); extra+=hs
    out+='<section style="page-break-after:always;width:1440px;height:900px;overflow:hidden">'+m+'</section>'
open('print.html','w').write('<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><title>UI设计</title><style>@page{size:1440px 900px;margin:0}html,body{margin:0;-webkit-print-color-adjust:exact;print-color-adjust:exact}section{width:1440px;height:900px}'+css+extra+'</style></head><body>'+cover+out+'</body></html>')
