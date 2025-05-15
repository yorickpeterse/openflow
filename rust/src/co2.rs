use crate::histogram::Histogram;

/// The lower bound of the histogram. All values up to this value are tracked in
/// the first bucket.
const LOW: u64 = 400;

/// The upper bound of the histogram. All values above this value are tracked in
/// the last bucket.
const HIGH: u64 = 1200;

/// The size/ppm per bucket.
const SIZE: u64 = 25;

/// The number of buckets to use for the CO2 histogram.
const BUCKETS: u64 = ((HIGH - LOW) / SIZE) + 1;

/// The last/maximum bucket.
const MAX_BUCKET: u64 = BUCKETS - 1;

fn co2_to_bucket(co2: u64) -> u64 {
    if co2 <= LOW {
        0
    } else if co2 <= HIGH {
        // Values such as 740 are rounded down to 700, while values such as 760 are
        // rounded up to 800. This ensures CO2 values are assigned to more accurate
        // buckets, instead of always being assigned to the lower bucket (as
        // integer division rounds down).
        ((co2 - LOW) + ((SIZE - 1) / 2)) / SIZE
    } else {
        MAX_BUCKET
    }
}

fn bucket_to_co2(bucket: u64) -> u64 {
    LOW + (bucket * SIZE)
}

pub(crate) struct Co2 {
    /// The histogram used to keep track of how many times certain CO2 levels
    /// are reported.
    histogram: Histogram,

    /// The current estimated CO2 level in parts-per-million (ppm).
    pub(crate) value: u64,
}

impl Co2 {
    pub(crate) fn new() -> Self {
        Self { histogram: Histogram::new(BUCKETS), value: 0 }
    }

    pub(crate) fn update(&mut self) {
        let sum_count = self.histogram.iter().enumerate().fold(
            (0_u64, 0_u64),
            |acc, buck_count| {
                (
                    acc.0
                        + ((LOW + (buck_count.0 as u64 * SIZE)) * buck_count.1),
                    acc.1 + buck_count.1,
                )
            },
        );

        let mean = if sum_count.1 > 0 { sum_count.0 / sum_count.1 } else { 0 };

        // The mean might not be a multiple of the bucket size, so we have to
        // get its nearest bucket, then convert that back to a rounded CO2
        // value.
        self.value = bucket_to_co2(co2_to_bucket(mean));
        self.histogram.reset();
    }

    pub(crate) fn add(&mut self, co2: u64) {
        self.histogram.increment(co2_to_bucket(co2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_co2_update_with_values_less_than_smallest_bucket() {
        let mut co2 = Co2::new();

        co2.update();
        assert_eq!(co2.value, 400);

        co2.add(390);
        co2.update();
        assert_eq!(co2.value, 400);
    }

    #[test]
    fn test_co2_update_without_any_samples() {
        let mut co2 = Co2::new();

        co2.update();
        assert_eq!(co2.value, 400);
    }

    #[test]
    fn test_co2_update_with_values_in_between_buckets() {
        let mut co2 = Co2::new();

        co2.add(720);
        co2.add(720);
        co2.update();
        assert_eq!(co2.value, 725);

        co2.add(735);
        co2.add(735);
        co2.update();
        assert_eq!(co2.value, 725);

        co2.add(760);
        co2.add(760);
        co2.update();
        assert_eq!(co2.value, 750);

        co2.add(775);
        co2.add(775);
        co2.update();
        assert_eq!(co2.value, 775);

        co2.add(785);
        co2.add(785);
        co2.update();
        assert_eq!(co2.value, 775);

        co2.add(740);
        co2.add(740);
        co2.update();
        assert_eq!(co2.value, 750);
    }

    #[test]
    fn test_co2_update_with_samples() {
        let mut co2 = Co2::new();

        co2.add(710);
        co2.add(780);
        co2.update();
        assert_eq!(co2.value, 725);

        co2.add(750);
        co2.add(803);
        co2.add(810);
        co2.update();
        assert_eq!(co2.value, 775);

        co2.add(1250);
        co2.add(1500);
        co2.update();
        assert_eq!(co2.value, 1200);

        co2.update();
        assert_eq!(co2.value, 400);

        co2.add(2500);
        co2.add(2500);
        co2.update();
        assert_eq!(co2.value, 1200);
    }

    #[test]
    fn test_co2_update_with_several_outliers() {
        let mut co2 = Co2::new();

        co2.add(725);
        co2.add(725);
        co2.add(725);
        co2.add(800);
        co2.add(875);
        co2.add(950);
        co2.add(950);
        co2.add(1200);
        co2.add(1200);
        co2.update();
        assert_eq!(co2.value, 900);
    }

    #[test]
    fn test_co2_update_with_a_short_increase() {
        let mut co2 = Co2::new();

        co2.add(675);
        co2.add(675);
        co2.add(675);
        co2.add(675);
        co2.add(1200);
        co2.add(1200);
        co2.add(800);
        co2.update();

        assert_eq!(co2.value, 850);
    }

    #[test]
    fn test_co2_update_with_an_even_distribution() {
        let mut co2 = Co2::new();

        co2.add(500);
        co2.add(500);
        co2.add(700);
        co2.add(700);
        co2.add(750);
        co2.add(750);
        co2.add(800);
        co2.add(800);
        co2.add(900);
        co2.add(900);

        co2.update();
        assert_eq!(co2.value, 725);
    }
}
