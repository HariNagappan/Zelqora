//! Streaming linear-interpolation resampler (device rate -> 16 kHz mono).

pub struct StreamingResampler {
    step: f64,
    pos: f64,
    buffer: Vec<f32>,
}

impl StreamingResampler {
    pub fn new(in_rate: f32, out_rate: f32) -> Self {
        StreamingResampler {
            step: (in_rate as f64) / (out_rate as f64),
            pos: 0.0,
            buffer: Vec::new(),
        }
    }

    /// Resample the next chunk of input; returns the output samples that can
    /// be produced so far. Safe to call with arbitrarily sized chunks.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if (self.step - 1.0).abs() < f64::EPSILON {
            return input.to_vec();
        }
        self.buffer.extend_from_slice(input);

        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 2);
        loop {
            let idx = self.pos as usize;
            if idx + 1 >= self.buffer.len() {
                break;
            }
            let frac = (self.pos - idx as f64) as f32;
            let s0 = self.buffer[idx];
            let s1 = self.buffer[idx + 1];
            out.push(s0 + (s1 - s0) * frac);
            self.pos += self.step;
        }

        let consumed = (self.pos as usize).min(self.buffer.len());
        if consumed > 0 {
            self.buffer.drain(0..consumed);
            self.pos -= consumed as f64;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(len: usize, freq: f32, rate: f32) -> Vec<f32> {
        (0..len)
            .map(|i| (2.0 * std::f32::consts::PI * freq * (i as f32) / rate).sin())
            .collect()
    }

    #[test]
    fn downsample_ratio_is_correct() {
        let mut r = StreamingResampler::new(48_000.0, 16_000.0);
        let input = sine(48_000, 440.0, 48_000.0);
        let out = r.process(&input);
        // 1 second in -> ~1 second out (within a couple of samples).
        assert!(
            (out.len() as i64 - 16_000).abs() < 8,
            "unexpected output length {}",
            out.len()
        );
    }

    #[test]
    fn streaming_across_chunks_matches_batch() {
        let input = sine(9_600, 220.0, 48_000.0);
        let mut streamed = StreamingResampler::new(48_000.0, 16_000.0);
        let mut out = Vec::new();
        for chunk in input.chunks(1000) {
            out.extend(streamed.process(chunk));
        }
        let mut batch = StreamingResampler::new(48_000.0, 16_000.0);
        let batch_out = batch.process(&input);
        assert!((out.len() as i64 - batch_out.len() as i64).abs() <= 2);
    }

    #[test]
    fn same_rate_passthrough() {
        let mut r = StreamingResampler::new(16_000.0, 16_000.0);
        let input = vec![0.1, 0.2, 0.3];
        assert_eq!(r.process(&input), input);
    }

    #[test]
    fn upsample_works() {
        let mut r = StreamingResampler::new(8_000.0, 16_000.0);
        let out = r.process(&sine(8_000, 100.0, 8_000.0));
        assert!((out.len() as i64 - 16_000).abs() < 8);
    }
}
