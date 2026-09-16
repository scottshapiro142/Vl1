//! A lock-free single-producer/single-consumer queue for reaching the audio
//! thread.
//!
//! Playing live means a keyboard thread and a MIDI callback have to hand events
//! to the audio callback, which must never block, allocate, or free. A mutex
//! would risk priority inversion and a channel would touch the allocator, so
//! this is a fixed-size ring of `Copy` values with two atomic cursors: the
//! producer only writes `write`, the consumer only writes `read`, and neither
//! ever waits for the other.

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Ring<T: Copy> {
    slots: Box<[UnsafeCell<MaybeUninit<T>>]>,
    mask: usize,
    write: AtomicUsize,
    read: AtomicUsize,
}

// Safety: exactly one producer and one consumer exist (they are created as a
// pair and neither is `Clone`), they touch disjoint slots, and the cursors are
// published with release/acquire ordering.
unsafe impl<T: Copy + Send> Sync for Ring<T> {}
unsafe impl<T: Copy + Send> Send for Ring<T> {}

/// The writing half. Lives on the keyboard or MIDI thread.
pub struct Producer<T: Copy + Send> {
    ring: Arc<Ring<T>>,
}

/// The reading half. Lives on the audio thread.
pub struct Consumer<T: Copy + Send> {
    ring: Arc<Ring<T>>,
}

/// Create a queue holding up to `capacity` items (rounded up to a power of two).
pub fn channel<T: Copy + Send>(capacity: usize) -> (Producer<T>, Consumer<T>) {
    let cap = capacity.next_power_of_two().max(2);
    let mut slots = Vec::with_capacity(cap);
    slots.resize_with(cap, || UnsafeCell::new(MaybeUninit::uninit()));

    let ring = Arc::new(Ring {
        slots: slots.into_boxed_slice(),
        mask: cap - 1,
        write: AtomicUsize::new(0),
        read: AtomicUsize::new(0),
    });

    (Producer { ring: ring.clone() }, Consumer { ring })
}

impl<T: Copy + Send> Producer<T> {
    /// Enqueue an item. Returns it back if the queue is full — which, for note
    /// events, means the audio thread has stalled badly enough that dropping
    /// them is the least bad option.
    pub fn push(&mut self, value: T) -> Result<(), T> {
        let write = self.ring.write.load(Ordering::Relaxed);
        let read = self.ring.read.load(Ordering::Acquire);
        if write.wrapping_sub(read) > self.ring.mask {
            return Err(value);
        }
        // Safety: this slot is outside the consumer's readable range until the
        // release store below publishes it.
        unsafe {
            (*self.ring.slots[write & self.ring.mask].get()).write(value);
        }
        self.ring
            .write
            .store(write.wrapping_add(1), Ordering::Release);
        Ok(())
    }
}

impl<T: Copy + Send> Consumer<T> {
    /// Dequeue the oldest item, if any. Never blocks or allocates.
    pub fn pop(&mut self) -> Option<T> {
        let read = self.ring.read.load(Ordering::Relaxed);
        let write = self.ring.write.load(Ordering::Acquire);
        if read == write {
            return None;
        }
        // Safety: the producer published this slot before advancing `write`.
        let value = unsafe { (*self.ring.slots[read & self.ring.mask].get()).assume_init() };
        self.ring
            .read
            .store(read.wrapping_add(1), Ordering::Release);
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_in_order() {
        let (mut tx, mut rx) = channel::<u32>(8);
        assert_eq!(rx.pop(), None);
        for i in 0..5 {
            tx.push(i).unwrap();
        }
        for i in 0..5 {
            assert_eq!(rx.pop(), Some(i));
        }
        assert_eq!(rx.pop(), None);
    }

    #[test]
    fn reports_full_rather_than_overwriting() {
        let (mut tx, mut rx) = channel::<u8>(4);
        for i in 0..4 {
            tx.push(i).unwrap();
        }
        assert_eq!(tx.push(99), Err(99));
        assert_eq!(rx.pop(), Some(0));
        // Space freed by the consumer becomes usable again.
        tx.push(99).unwrap();
    }

    #[test]
    fn survives_a_concurrent_producer() {
        let (mut tx, mut rx) = channel::<usize>(64);
        let sender = std::thread::spawn(move || {
            for i in 0..10_000 {
                while tx.push(i).is_err() {
                    std::hint::spin_loop();
                }
            }
        });

        let mut expected = 0;
        while expected < 10_000 {
            if let Some(v) = rx.pop() {
                assert_eq!(v, expected);
                expected += 1;
            }
        }
        sender.join().unwrap();
    }
}
