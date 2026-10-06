//! Sound preview playback (desktop only).

use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink};
use std::io::Cursor;

#[derive(Default)]
pub struct Player {
    stream: Option<OutputStream>,
    sink: Option<Sink>,
}

impl Player {
    /// Plays encoded wav / mp3 `bytes`, replacing whatever is playing.
    pub fn play(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        self.stop();
        if self.stream.is_none() {
            let mut s = OutputStreamBuilder::open_default_stream().map_err(|e| format!("no audio device: {e}"))?;
            s.log_on_drop(false);
            self.stream = Some(s);
        }
        let dec = Decoder::new(Cursor::new(bytes)).map_err(|e| format!("cannot decode: {e}"))?;
        let sink = Sink::connect_new(self.stream.as_ref().map(|s| s.mixer()).ok_or("no audio device")?);
        sink.append(dec);
        self.sink = Some(sink);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(s) = self.sink.take() {
            s.stop();
        }
    }

    pub fn is_playing(&self) -> bool {
        self.sink.as_ref().is_some_and(|s| !s.empty())
    }
}
