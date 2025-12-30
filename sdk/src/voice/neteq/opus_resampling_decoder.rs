use neteq::codec::{AudioDecoder, OpusDecoder};
use crate::error::SdkError;
use crate::voice::resampler::AudioResampler;

/// Custom decoder that wraps Opus decoding with optional resampling
pub(crate) struct OpusResamplingDecoder {
    opus_decoder: OpusDecoder,
    resampler: Option<AudioResampler>,
    target_sample_rate: u32,
}

impl OpusResamplingDecoder {
    pub fn new(
        source_sample_rate: u32,
        target_sample_rate: u32,
        channels: u8,
        frame_size: u32,
    ) -> Result<Self, SdkError> {
        // Create Opus decoder at 48kHz
        let opus_decoder = OpusDecoder::new(source_sample_rate, channels)
            .map_err(|e| SdkError::DecoderError(e.to_string()))?;

        // Create resampler if needed
        let resampler = if target_sample_rate != source_sample_rate {
            Some(AudioResampler::new(source_sample_rate, target_sample_rate, frame_size)?)
        } else {
            None
        };

        Ok(Self {
            opus_decoder,
            resampler,
            target_sample_rate,
        })
    }
}

impl AudioDecoder for OpusResamplingDecoder {
    fn sample_rate(&self) -> u32 { self.target_sample_rate }

    fn channels(&self) -> u8 { 1 }

    fn decode(&mut self, encoded: &[u8]) -> neteq::Result<Vec<f32>> {
        let decoded = self.opus_decoder.decode(encoded)?;
        match &mut self.resampler {
            None => { Ok(decoded) }
            Some(resampler) => {
                resampler
                    .resample(decoded)
                    .map_err(|e| { neteq::NetEqError::DecoderError(e.to_string()) })
            }
        }
    }
}

// SAFETY: OpusResamplingDecoder is only accessed through a Mutex,
// ensuring exclusive access. Internal types are Send-safe.
unsafe impl Send for OpusResamplingDecoder {}