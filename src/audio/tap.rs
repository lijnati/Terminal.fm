//! A pass-through [`Source`] that copies decoded PCM to the visualizer.
//!
//! The tap sits directly after the decoder, *before* the player's volume and
//! pause stages, so the visualizer sees the exact samples being played,
//! independent of volume. Samples flow through a bounded lock-free SPSC ring
//! buffer; when the consumer falls behind, new samples are dropped rather than
//! ever blocking the audio thread.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::source::SeekError;
use rodio::{ChannelCount, Sample, SampleRate, Source};

/// Ring capacity in samples: ~0.35 s of 48 kHz stereo.
pub const RING_CAPACITY: usize = 32_768;
const BATCH: usize = 256;

/// Shared slot holding the ring producer. Exactly one `Tap` owns it at a time;
/// a newer tap picks it up when the previous one is dropped.
type ProducerSlot = Arc<Mutex<Option<rtrb::Producer<f32>>>>;

/// Consumer side, owned by the UI thread.
pub struct TapReader {
    consumer: rtrb::Consumer<f32>,
    format: Arc<AtomicU64>,
}

/// Creates the shared ring. Use [`TapFactory::wrap`] for each new track.
pub fn tap_pair() -> (TapFactory, TapReader) {
    let (producer, consumer) = rtrb::RingBuffer::new(RING_CAPACITY);
    let format = Arc::new(AtomicU64::new(0));
    (
        TapFactory { slot: Arc::new(Mutex::new(Some(producer))), format: format.clone() },
        TapReader { consumer, format },
    )
}

fn pack(channels: u16, rate: u32) -> u64 {
    (u64::from(rate) << 16) | u64::from(channels)
}

impl TapReader {
    /// Moves all pending samples into `out` (cleared first). Returns `(channels, sample_rate)`.
    pub fn drain(&mut self, out: &mut Vec<f32>) -> (u16, u32) {
        out.clear();
        let n = self.consumer.slots();
        if let Ok(chunk) = self.consumer.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            out.extend_from_slice(a);
            out.extend_from_slice(b);
            chunk.commit_all();
        }
        let f = self.format.load(Ordering::Relaxed);
        ((f & 0xFFFF) as u16, (f >> 16) as u32)
    }
}

#[derive(Clone)]
pub struct TapFactory {
    slot: ProducerSlot,
    format: Arc<AtomicU64>,
}

impl TapFactory {
    pub fn wrap<S: Source>(&self, inner: S) -> Tap<S> {
        Tap {
            inner,
            slot: self.slot.clone(),
            producer: None,
            format: self.format.clone(),
            batch: Vec::with_capacity(BATCH),
        }
    }
}

pub struct Tap<S> {
    inner: S,
    slot: ProducerSlot,
    producer: Option<rtrb::Producer<f32>>,
    format: Arc<AtomicU64>,
    batch: Vec<f32>,
}

impl<S: Source> Tap<S> {
    fn flush(&mut self) {
        if self.producer.is_none() {
            // Never block the audio thread: try once per batch.
            if let Ok(mut slot) = self.slot.try_lock() {
                self.producer = slot.take();
            }
        }
        if let Some(p) = &mut self.producer {
            self.format
                .store(pack(self.inner.channels().get(), self.inner.sample_rate().get()), Ordering::Relaxed);
            let n = p.slots().min(self.batch.len());
            // Keep whole frames when we have to drop.
            let ch = self.inner.channels().get() as usize;
            let n = n - n % ch.max(1);
            let _ = p.push_partial_slice(&self.batch[..n]);
        }
        self.batch.clear();
    }
}

impl<S> Drop for Tap<S> {
    fn drop(&mut self) {
        if let Some(p) = self.producer.take() {
            if let Ok(mut slot) = self.slot.lock() {
                *slot = Some(p);
            }
        }
    }
}

impl<S: Source> Iterator for Tap<S> {
    type Item = Sample;

    #[inline]
    fn next(&mut self) -> Option<Sample> {
        let s = self.inner.next();
        match s {
            Some(v) => {
                self.batch.push(v);
                if self.batch.len() >= BATCH {
                    self.flush();
                }
            }
            None => self.flush(),
        }
        s
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source> Source for Tap<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.batch.clear();
        self.inner.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;

    fn buf(samples: Vec<f32>) -> SamplesBuffer {
        SamplesBuffer::new(NonZero::new(2).unwrap(), NonZero::new(48_000).unwrap(), samples)
    }

    #[test]
    fn passes_samples_through_unchanged_and_copies_them() {
        let (factory, mut reader) = tap_pair();
        let input: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        let out: Vec<f32> = factory.wrap(buf(input.clone())).collect();
        assert_eq!(out, input);
        let mut seen = Vec::new();
        let (ch, sr) = reader.drain(&mut seen);
        assert_eq!((ch, sr), (2, 48_000));
        assert_eq!(seen, input);
    }

    #[test]
    fn drops_when_full_without_blocking() {
        let (factory, mut reader) = tap_pair();
        let n = RING_CAPACITY * 2;
        let out: Vec<f32> = factory.wrap(buf(vec![0.25; n])).collect();
        assert_eq!(out.len(), n, "playback must receive every sample");
        let mut seen = Vec::new();
        reader.drain(&mut seen);
        assert!(seen.len() <= RING_CAPACITY && seen.len() > RING_CAPACITY / 2);
        assert_eq!(seen.len() % 2, 0, "only whole frames");
    }

    #[test]
    fn producer_is_handed_to_the_next_tap() {
        let (factory, mut reader) = tap_pair();
        let first = factory.wrap(buf(vec![1.0; 512]));
        let second = factory.wrap(buf(vec![2.0; 512]));
        // `first` is still alive while `second` starts: second gets nothing yet.
        let mut first = first;
        first.next();
        let _: Vec<f32> = first.by_ref().collect();
        drop(first);
        let _: Vec<f32> = second.collect();
        let mut seen = Vec::new();
        reader.drain(&mut seen);
        assert!(seen.contains(&1.0) && seen.contains(&2.0));
    }
}
