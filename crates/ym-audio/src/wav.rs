//! Minimal WAV encoding: the demo catalogue's audio is synthesised and handed to the normal
//! decoder as a WAV file, and the tests build their fixtures the same way.

/// 16-bit PCM WAV from interleaved samples in -1.0..=1.0.
pub fn encode(samples: &[f32], channels: u16, rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let block_align = channels * 2;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::sync::Arc;

    /// A constant-level mono clip: any dip in the output is a defect in the engine.
    pub fn dc(level: f32, secs: f32, rate: u32) -> Arc<[u8]> {
        let n = (secs * rate as f32) as usize;
        super::encode(&vec![level; n], 1, rate).into()
    }

    pub fn sine(hz: f32, secs: f32, rate: u32, channels: u16) -> Arc<[u8]> {
        let frames = (secs * rate as f32) as usize;
        let mut samples = Vec::with_capacity(frames * usize::from(channels));
        for i in 0..frames {
            let s = 0.5 * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin();
            samples.extend(std::iter::repeat_n(s, usize::from(channels)));
        }
        super::encode(&samples, channels, rate).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_well_formed() {
        let w = encode(&[0.0, 0.5, -0.5, 1.0], 2, 44_100);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize, w.len() - 8);
        assert_eq!(u16::from_le_bytes(w[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 44_100);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 8);
        assert_eq!(w.len(), 44 + 8);
        let sample = |i: usize| i16::from_le_bytes(w[44 + 2 * i..46 + 2 * i].try_into().unwrap());
        assert_eq!([sample(0), sample(1), sample(2), sample(3)], [0, 16384, -16384, 32767]);
    }
}
