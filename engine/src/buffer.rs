//! Single-producer / single-consumer ring buffer for f32 stereo PCM.

pub use ringbuf::traits::{Consumer, Observer, Producer, Split};
pub use ringbuf::wrap::caching::Caching;
use ringbuf::HeapRb;

pub type SampleProducer = Caching<std::sync::Arc<HeapRb<f32>>, true, false>;
pub type SampleConsumer = Caching<std::sync::Arc<HeapRb<f32>>, false, true>;

pub fn make_ring(capacity_samples: usize) -> (SampleProducer, SampleConsumer) {
    let rb = HeapRb::<f32>::new(capacity_samples);
    rb.split()
}
