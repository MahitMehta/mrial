use flacenc::{config::Encoder, source::Source};

pub struct AudioEncoder {
    encoder: Encoder,
    channels: usize,
    bits_per_sample: usize,
    sample_rate: usize,
}

impl AudioEncoder {
    pub fn new(channels: usize, bits_per_sample: usize, sample_rate: usize) -> Self {
        Self {
            encoder: flacenc::config::Encoder::default(),
            channels,
            bits_per_sample,
            sample_rate,
        }
    }

    pub fn encode(&self, samples: &[i32]) {
        println!("Block {}", self.encoder.block_sizes[0]);
        let source = flacenc::source::MemSource::from_samples(
            samples, 
            self.channels, 
            self.bits_per_sample, 
            self.sample_rate
        );
        
        let flac_stream = flacenc::encode_with_fixed_block_size(
            &self.encoder, 
            source, 
            self.encoder.block_sizes[0]
        ).expect("Audio Encoding failed.");

        println!("count: {}", flac_stream.frame_count());
    }
}