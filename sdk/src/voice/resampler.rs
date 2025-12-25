use rubato::{FftFixedIn, Resampler};
use thiserror::Error;

/// Audio resampler for converting between sample rates
pub struct AudioResampler {
    /// FFT-based resampler
    resampler: FftFixedIn<f32>,
    
    /// Pre-allocated output buffer for zero-allocation resampling
    output_buffer: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, Error)]
pub enum ResamplerError {
    #[error("Resampler initialization error: {0}")]
    InitializationError(String),

    #[error("Resampling error: {0}")]
    ResamplingError(String),
}

impl AudioResampler {
    /// Create a new resampler
    ///
    /// # Arguments
    /// * `source_sample_rate` - Input audio sample rate
    /// * `target_sample_rate` - Output audio sample rate
    /// * `frame_size` - Number of samples per frame (mono)
    pub fn new(
        source_sample_rate: u32,
        target_sample_rate: u32,
        frame_size: u32,
    ) -> Result<Self, ResamplerError> {
        let resampler = FftFixedIn::<f32>::new(
            source_sample_rate as usize,
            target_sample_rate as usize,
            frame_size as usize,
            2, // sub_chunks (quality/performance balance)
            1, // mono channel
        ).map_err(|e| ResamplerError::InitializationError(
            format!("Failed to create resampler: {}", e)
        ))?;

        // Pre-allocate output buffer for zero-allocation processing
        let output_buffer = resampler.output_buffer_allocate(true);

        Ok(Self {
            resampler,
            output_buffer,
        })
    }

    /// Resample audio data
    ///
    /// # Arguments
    /// * `input` - Input audio samples at source sample rate
    ///
    /// # Returns
    /// Resampled audio samples at target sample rate
    pub fn resample(&mut self, input: Vec<f32>) -> Result<Vec<f32>, ResamplerError> {
        let (_, resampled_size) = self.resampler
            .process_into_buffer(&[&input], &mut self.output_buffer, None)
            .map_err(|e| ResamplerError::ResamplingError(
                format!("Resampling failed: {}", e)
            ))?;

        // Extract resampled samples from mono channel
        Ok(self.output_buffer[0][0..resampled_size].to_vec())
    }
}
