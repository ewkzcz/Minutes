//! 音频输入：麦克风（cpal，重采样到 16k 单声道 PCM16）或 WAV 文件（演示 / 联调）。

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

pub const SAMPLE_RATE: u32 = 16000;
const CHUNK_SAMPLES: usize = 1600; // 100ms

pub type LevelFn = Arc<dyn Fn(f32) + Send + Sync>;

/// 暂停时发送静音（保持识别连接不断开），并上报音量。
fn emit_chunk(chunk: &[i16], tx: &UnboundedSender<Vec<u8>>, paused: &AtomicBool, level: &LevelFn) {
    let is_paused = paused.load(Ordering::Relaxed);
    let mut bytes = Vec::with_capacity(chunk.len() * 2);
    let mut sum = 0f64;
    for &s in chunk {
        let v = if is_paused { 0 } else { s };
        sum += (v as f64 / 32768.0).powi(2);
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    level(((sum / chunk.len().max(1) as f64).sqrt() * 4.0).min(1.0) as f32);
    let _ = tx.send(bytes);
}

pub fn list_devices() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|it| it.filter_map(|d| d.description().ok().map(|x| x.name().to_string())).collect())
        .unwrap_or_default()
}

/// 音频来源：麦克风（自己说话）/ 系统声音（耳机或扬声器里播放的对方声音）/ 两者混合。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Mic,
    System,
    Both,
}

impl Source {
    pub fn parse(s: &str) -> Source {
        match s {
            "mic" => Source::Mic,
            "system" => Source::System,
            _ => Source::Both,
        }
    }
}

type Queue = Arc<Mutex<VecDeque<f32>>>;

/// 采集句柄：drop 时停止全部采集线程。
pub struct CaptureHandle {
    stop: Arc<AtomicBool>,
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// 把设备回调里的多声道任意采样，下混为单声道并线性重采样到 16k，写入队列。
struct Feeder {
    ratio: f64,
    buf: Vec<f32>,
    pos: f64,
    queue: Queue,
}

impl Feeder {
    fn push(&mut self, mono: &[f32]) {
        self.buf.extend_from_slice(mono);
        let mut out = Vec::new();
        while self.pos + 1.0 < self.buf.len() as f64 {
            let i = self.pos as usize;
            let f = (self.pos - i as f64) as f32;
            out.push(self.buf[i] * (1.0 - f) + self.buf[i + 1] * f);
            self.pos += self.ratio;
        }
        let drop = (self.pos as usize).min(self.buf.len());
        self.buf.drain(..drop);
        self.pos -= drop as f64;
        self.queue.lock().unwrap().extend(out);
    }
}

fn build<T>(device: &cpal::Device, cfg: cpal::StreamConfig, mut feeder: Feeder) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = cfg.channels as usize;
    let stream = device
        .build_input_stream(
            cfg,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mono: Vec<f32> = data.chunks(channels).map(|f| f.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / channels as f32).collect();
                feeder.push(&mono);
            },
            |e| eprintln!("音频采集错误: {e}"),
            None,
        )
        .map_err(|e| anyhow!("{e}"))?;
    Ok(stream)
}

/// 在独立线程里打开一路采集（麦克风，或对默认输出设备做回环 = 系统声音），成功后阻塞持有 stream 直到停止。
fn spawn_capture(loopback: bool, device_name: String, queue: Queue, stop: Arc<AtomicBool>) -> Result<()> {
    let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
    std::thread::spawn(move || {
        let setup = || -> Result<cpal::Stream> {
            let host = cpal::default_host();
            let device = if loopback {
                host.default_output_device().ok_or_else(|| anyhow!("未找到输出设备"))?
            } else if device_name.is_empty() {
                host.default_input_device().ok_or_else(|| anyhow!("未找到麦克风设备"))?
            } else {
                host.input_devices()
                    .map_err(|e| anyhow!("{e}"))?
                    .find(|d| d.description().map(|x| x.name() == device_name).unwrap_or(false))
                    .or_else(|| host.default_input_device())
                    .ok_or_else(|| anyhow!("未找到麦克风设备"))?
            };
            let supported = if loopback { device.default_output_config() } else { device.default_input_config() }.map_err(|e| anyhow!("{e}"))?;
            let cfg = supported.config();
            let feeder = Feeder { ratio: cfg.sample_rate as f64 / SAMPLE_RATE as f64, buf: vec![], pos: 0.0, queue };
            let stream = match supported.sample_format() {
                SampleFormat::F32 => build::<f32>(&device, cfg, feeder)?,
                SampleFormat::I16 => build::<i16>(&device, cfg, feeder)?,
                SampleFormat::U16 => build::<u16>(&device, cfg, feeder)?,
                SampleFormat::I32 => build::<i32>(&device, cfg, feeder)?,
                f => return Err(anyhow!("不支持的采样格式 {f:?}")),
            };
            stream.play().map_err(|e| anyhow!("{e}"))?;
            Ok(stream)
        };
        match setup() {
            Ok(stream) => {
                let _ = ready_tx.send(Ok(()));
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(60));
                }
                drop(stream);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        }
    });
    ready_rx.recv().map_err(|_| anyhow!("采集线程异常退出"))?
}

/// 启动采集：按来源打开一路或两路，混音后每 100ms 输出一包 16k PCM16。
/// 返回句柄与警告（例如系统声音不可用而降级为仅麦克风）。
pub fn start_capture(source: Source, device_name: &str, tx: UnboundedSender<Vec<u8>>, paused: Arc<AtomicBool>, level: LevelFn) -> Result<(CaptureHandle, Vec<String>)> {
    let stop = Arc::new(AtomicBool::new(false));
    let mut queues: Vec<Queue> = vec![];
    let mut warnings = vec![];
    let mut errors = vec![];
    for (loopback, wanted) in [(false, source != Source::System), (true, source != Source::Mic)] {
        if !wanted {
            continue;
        }
        let q: Queue = Arc::new(Mutex::new(VecDeque::new()));
        match spawn_capture(loopback, device_name.to_string(), q.clone(), stop.clone()) {
            Ok(()) => queues.push(q),
            Err(e) => errors.push(format!("{}：{e}", if loopback { "系统声音" } else { "麦克风" })),
        }
    }
    if queues.is_empty() {
        stop.store(true, Ordering::Relaxed);
        return Err(anyhow!("{}", errors.join("；")));
    }
    if !errors.is_empty() {
        warnings.push(format!("部分音源不可用，已自动降级：{}（系统声音需 macOS 14.6+ 并授权「系统音频录制」）", errors.join("；")));
    }
    let stop2 = stop.clone();
    std::thread::spawn(move || {
        let mut next = Instant::now();
        while !stop2.load(Ordering::Relaxed) {
            next += Duration::from_millis(100);
            let mut mix = vec![0f32; CHUNK_SAMPLES];
            for q in &queues {
                let mut g = q.lock().unwrap();
                while g.len() > CHUNK_SAMPLES * 6 {
                    g.pop_front(); // 积压过多则丢弃最旧数据，保持低延迟
                }
                for m in mix.iter_mut() {
                    match g.pop_front() {
                        Some(v) => *m += v,
                        None => break,
                    }
                }
            }
            let chunk: Vec<i16> = mix.iter().map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
            emit_chunk(&chunk, &tx, &paused, &level);
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                next = now;
            }
        }
    });
    Ok((CaptureHandle { stop }, warnings))
}

/// 读取 16k 单声道 16 位 PCM WAV 的数据段。
pub fn read_wav_pcm(path: &str) -> Result<Vec<u8>> {
    let data = std::fs::read(path)?;
    if data.len() < 44 || &data[0..4] != b"RIFF" {
        return Err(anyhow!("不是 WAV 文件"));
    }
    let mut i = 12;
    while i + 8 <= data.len() {
        let id = &data[i..i + 4];
        let size = u32::from_le_bytes(data[i + 4..i + 8].try_into().unwrap()) as usize;
        if id == b"data" {
            let end = (i + 8 + size).min(data.len());
            return Ok(data[i + 8..end].to_vec());
        }
        i += 8 + size + (size & 1);
    }
    Err(anyhow!("WAV 缺少 data 段"))
}

/// 按实时速度把 PCM 推入通道，结束后再补 1.5 秒静音让识别收尾。
pub async fn feed_pcm(pcm: Vec<u8>, tx: UnboundedSender<Vec<u8>>, paused: Arc<AtomicBool>, level: LevelFn, mut stop: tokio::sync::watch::Receiver<bool>) {
    let chunk_bytes = CHUNK_SAMPLES * 2;
    let mut all = pcm;
    all.extend(std::iter::repeat(0u8).take(chunk_bytes * 15));
    for c in all.chunks(chunk_bytes) {
        let samples: Vec<i16> = c.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        emit_chunk(&samples, &tx, &paused, &level);
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
            _ = stop.changed() => return,
        }
    }
    // 文件播完后持续静音，直到用户结束
    loop {
        emit_chunk(&vec![0i16; CHUNK_SAMPLES], &tx, &paused, &level);
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
            _ = stop.changed() => return,
        }
    }
}
