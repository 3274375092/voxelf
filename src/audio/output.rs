use anyhow::{Context, Result};
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamBuilder, Sink};

/// TTS 音频播放器。Sink 内部是通道,方法都是 &self。
pub struct Player {
    _stream: OutputStream,
    sink: Sink,
}

impl Player {
    pub fn new() -> Result<Self> {
        let stream = OutputStreamBuilder::open_default_stream().context("打开音频输出设备失败")?;
        let sink = Sink::connect_new(stream.mixer());
        Ok(Self { _stream: stream, sink })
    }

    /// 播放一段 PCM 音频(单声道 f32)。会先停掉正在播的。
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
