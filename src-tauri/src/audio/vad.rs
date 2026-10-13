//! Tiny RMS-based voice-activity detector with hysteresis.
//! Frames are 20 ms of 16 kHz audio (320 samples).

/// Frame length in samples (20 ms at 16 kHz).
pub const FRAME_LEN: usize = 320;

/// Silence frames needed to close a speech segment (~1200 ms).
const HANGOVER_FRAMES: usize = 60;
/// Voiced frames needed to confirm speech onset (~60 ms).
const ONSET_FRAMES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadEvent {
    /// No speech in progress.
    Silence,
    /// Speech just started (segment should begin).
    SpeechStart,
    /// Speech continues.
    Speech,
    /// Speech ended (segment should be finalized).
    SpeechEnd,
}

pub struct Vad {
    speaking: bool,
    onset: usize,
    hangover_left: usize,
}

impl Default for Vad {
    fn default() -> Self {
        Self::new()
    }
}

impl Vad {
    pub fn new() -> Self {
        Vad {
            speaking: false,
            onset: 0,
            hangover_left: 0,
        }
    }

    pub fn reset(&mut self) {
        *self = Vad::new();
    }

    pub fn process(&mut self, frame: &[f32], threshold: f32) -> VadEvent {
        let voiced = rms(frame) >= threshold;
        match (self.speaking, voiced) {
            (false, true) => {
                self.onset += 1;
                if self.onset >= ONSET_FRAMES {
                    self.speaking = true;
                    self.hangover_left = HANGOVER_FRAMES;
                    VadEvent::SpeechStart
                } else {
                    VadEvent::Silence
                }
            }
            (true, true) => {
                self.hangover_left = HANGOVER_FRAMES;
                VadEvent::Speech
            }
            (true, false) => {
                self.hangover_left = self.hangover_left.saturating_sub(1);
                if self.hangover_left == 0 {
                    self.speaking = false;
                    self.onset = 0;
                    VadEvent::SpeechEnd
                } else {
                    VadEvent::Speech
                }
            }
            (false, false) => {
                self.onset = 0;
                VadEvent::Silence
            }
        }
    }
}

pub fn rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum: f32 = frame.iter().map(|s| s * s).sum();
    (sum / frame.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loud() -> Vec<f32> {
        (0..FRAME_LEN)
            .map(|i| if i % 2 == 0 { 0.2 } else { -0.2 })
            .collect()
    }

    fn quiet() -> Vec<f32> {
        vec![0.0; FRAME_LEN]
    }

    #[test]
    fn detects_speech_start_after_onset() {
        let mut vad = Vad::new();
        assert_eq!(vad.process(&loud(), 0.01), VadEvent::Silence); // onset 1
        assert_eq!(vad.process(&loud(), 0.01), VadEvent::Silence); // onset 2
        assert_eq!(vad.process(&loud(), 0.01), VadEvent::SpeechStart);
        assert_eq!(vad.process(&loud(), 0.01), VadEvent::Speech);
    }

    #[test]
    fn speech_ends_after_hangover() {
        let mut vad = Vad::new();
        for _ in 0..5 {
            vad.process(&loud(), 0.01);
        }
        // Feed silence: still "Speech" during the hangover window.
        for i in 0..HANGOVER_FRAMES - 1 {
            let ev = vad.process(&quiet(), 0.01);
            assert_eq!(ev, VadEvent::Speech, "frame {i} should still be speech");
        }
        assert_eq!(vad.process(&quiet(), 0.01), VadEvent::SpeechEnd);
        assert_eq!(vad.process(&quiet(), 0.01), VadEvent::Silence);
    }

    #[test]
    fn short_blip_does_not_trigger() {
        let mut vad = Vad::new();
        assert_eq!(vad.process(&loud(), 0.01), VadEvent::Silence);
        assert_eq!(vad.process(&quiet(), 0.01), VadEvent::Silence);
    }

    #[test]
    fn rms_sanity() {
        assert!(rms(&loud()) > 0.1);
        assert_eq!(rms(&quiet()), 0.0);
    }
}
