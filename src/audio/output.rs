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
    /// 注: 当前流式管线用 queue,此接口留给未来的语音打断(barge-in)。
    #[allow(dead_code)]
    pub fn play(&self, samples: Vec<f32>, sample_rate: u32) {
        self.sink.stop();
        self.queue(samples, sample_rate);
    }

    /// 把一段 PCM 音频追加到播放队列(不打断当前播放),TTS 流式用。
    pub fn queue(&self, samples: Vec<f32>, sample_rate: u32) {
        let buf = SamplesBuffer::new(1, sample_rate, samples);
        self.sink.append(buf);
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        self.sink.stop();
    }

    pub fn is_idle(&self) -> bool {
        self.sink.empty()
    }
}
