use anyhow::{Context, Result};
use rodio::{buffer::SamplesBuffer, OutputStream, OutputStreamHandle, Sink};

/// TTS 音频播放器。线程安全使用:Sink 内部有锁。
pub struct Player {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    sink: Sink,
}

impl Player {
    pub fn new() -> Result<Self> {
        let (stream, handle) = OutputStream::try_default().context("打开音频输出设备失败")?;
        let sink = Sink::try_new(&handle).context("创建播放器失败")?;
        Ok(Self { _stream: stream, handle, sink })
    }

    /// 播放一段 PCM 音频(单声道 f32)。
    pub fn play(&self, samples: Vec<f32>, sample_rate: u32) {
        self.sink.stop();
        let buf = SamplesBuffer::new(1, sample_rate, samples);
        self.sink.append(buf);
    }

    pub fn stop(&self) {
        self.sink.stop();
    }

    pub fn is_idle(&self) -> bool {
        self.sink.empty()
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new().expect("初始化播放器失败")
    }
}
