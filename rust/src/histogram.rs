use std::iter::Iterator;

pub(crate) struct Histogram {
    buckets: Vec<u64>,
}

impl Histogram {
    pub(crate) fn new(buckets: u64) -> Self {
        Self { buckets: vec![0; buckets as usize] }
    }

    pub(crate) fn get(&self, bucket: u64) -> u64 {
        self.buckets[bucket as usize]
    }

    pub(crate) fn remove(&mut self, bucket: u64) {
        self.buckets[bucket as usize] = 0;
    }

    pub(crate) fn increment(&mut self, bucket: u64) {
        self.buckets[bucket as usize] += 1;
    }

    pub(crate) fn reset(&mut self) {
        for val in &mut self.buckets {
            *val = 0;
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = u64> {
        self.buckets.iter().cloned()
    }
}
