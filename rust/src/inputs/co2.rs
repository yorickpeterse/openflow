use crate::co2::Co2;
use crate::logger::Logger;
use crate::metrics::Metrics;
use crate::more_sense::{Error, MoreSense};
use crate::state::{StateRef, Status};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// The interval (in seconds) at which to sample the CO2 sensors.
const SAMPLE_INTERVAL: u64 = 30;

/// The time (in seconds) to wait before reducing the ventilation speed in
/// response to a reduction in CO2 levels.
const REDUCE_WAIT_TIME: u64 = 1800;

/// The interval (in seconds) at which to calculate a new CO2 value based on the
/// gathered samples.
const UPDATE_INTERVAL: u64 = 900;

/// How many samples we should gather before updating the CO2 levels.
const SAMPLES: u64 = UPDATE_INTERVAL / SAMPLE_INTERVAL;

/// The state of a single CO2 sensor.
pub(crate) struct Sensor {
    name: String,
    client: MoreSense,
    co2: Co2,

    /// The ventilation status of the room this sensor belongs to.
    status: Status,

    /// The time at which the current status was produced.
    last_status_change: Instant,

    /// The CO2 at the time of the last status change.
    last_change_co2: u64,

    /// The lowest CO2 value at which to start active ventilation.
    pub(crate) co2_minimum: u64,
}

impl Sensor {
    pub(crate) fn new(name: String, host: String) -> Self {
        Self {
            name: name,
            client: MoreSense::new(host),
            co2: Co2::new(),
            status: Status::Default,
            last_status_change: Instant::now(),
            last_change_co2: 0,
            co2_minimum: 700,
        }
    }

    fn sample(&mut self) -> Result<(), Error> {
        self.co2.add(self.client.co2()?);
        Ok(())
    }

    fn value(&self) -> u64 {
        self.co2.value
    }

    fn update_co2(&mut self) -> u64 {
        self.co2.update();
        self.co2.value
    }

    fn update_status(&mut self, status: Status) {
        self.status = status;
        self.last_status_change = Instant::now();
        self.last_change_co2 = self.co2.value;
    }

    fn should_reduce(&self, after: Duration) -> bool {
        // If we reduce levels this much, we can safely reduce ventilation
        // speeds right away. This won't reduce speeds too quickly, as we only
        // periodically reach this point.
        if (self.last_change_co2 - self.co2.value >= 200)
            || self.co2.value <= 600
        {
            return true;
        }

        // After the timeout, if we're below 700 ppm we just reduce anyway,
        // regardless of the previous value. This way if we go from e.g. 750 to
        // 675, we don't keep running at the 750 ppm speed for way too long.
        self.last_status_change.elapsed() >= after
            && (self.last_change_co2 - self.co2.value >= 100
                || self.co2.value < 700)
    }
}

/// A thread monitoring a set of CO2 sensors, adjusting ventilation based on the
/// CO2 levels.
pub(crate) struct Input {
    state: StateRef,
    logger: Logger,
    metrics: Arc<Metrics>,

    /// The room names and their corresponding sensors to monitor.
    sensors: Vec<Sensor>,

    /// The time between CO2 samples.
    sample_interval: Duration,

    /// The amount of time to wait after an update before reducing the
    /// ventilation speed of a room.
    reduce_wait_time: Duration,
}

impl Input {
    pub(crate) fn new(
        state: StateRef,
        logger: Logger,
        metrics: Arc<Metrics>,
    ) -> Self {
        Self {
            state,
            logger,
            metrics,
            sensors: Vec::new(),
            sample_interval: Duration::from_secs(SAMPLE_INTERVAL),
            reduce_wait_time: Duration::from_secs(REDUCE_WAIT_TIME),
        }
    }

    pub(crate) fn add_sensor(&mut self, sensor: Sensor) {
        self.sensors.push(sensor);
    }

    pub(crate) fn run(mut self) {
        thread::Builder::new()
            .name("co2 input".to_string())
            .spawn(move || {
                loop {
                    self.run_iteration();
                }
            })
            .unwrap();
    }

    fn run_iteration(&mut self) {
        debug_assert!(SAMPLES > 0);

        for _ in 0..SAMPLES {
            thread::sleep(self.sample_interval);
            self.sample();
        }

        self.update();
    }

    fn update(&mut self) {
        let mut updates = HashMap::new();

        for sen in &mut self.sensors {
            let new = sen.update_co2();
            let min = sen.co2_minimum;
            let status = if new >= 1000 {
                Status::Maximum
            } else if new >= 900 {
                Status::High
            } else if new >= 800 {
                Status::MediumHigh
            } else if new >= 750 {
                Status::Medium
            } else if new >= min
                || (new >= (min - 50) && sen.last_change_co2 >= min)
            {
                Status::Low
            } else {
                Status::Default
            };

            self.metrics.add("co2_room", |m| {
                m.tag("room", &sen.name);
                m.field("ppm", new);
            });

            if status >= sen.status || sen.should_reduce(self.reduce_wait_time)
            {
                sen.update_status(status.clone());
                updates.insert(sen.name.clone(), status);
            }
        }

        self.apply(updates);
    }

    fn sample(&mut self) {
        for sensor in &mut self.sensors {
            if let Err(err) = sensor.sample() {
                self.logger.error(format!(
                    "{}: failed to read the CO2 value, {}",
                    sensor.name, err
                ));
            }
        }
    }

    fn apply(&mut self, updates: HashMap<String, Status>) {
        self.state.update(move |state| {
            for (name, status) in updates {
                // The room must be present at this point, and there's nothing
                // we can do if it isn't.
                let room = state.rooms.get_mut(&name).unwrap();

                match &room.status {
                    Status::Humid | Status::Button(_) => {}
                    _ => room.update(status),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;
    use crate::test::{logger, metrics, state};
    use mockito::Server;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn sensor(server: &mut Server, name: &str, samples: Vec<u64>) -> Sensor {
        let idx = AtomicUsize::new(0);

        server
            .mock("GET", "/VALUES")
            .expect(samples.len())
            .with_status(200)
            .with_body_from_request(move |_| {
                let old = idx.fetch_add(1, Ordering::Relaxed);
                let val = samples.get(old).cloned().unwrap_or(0);

                format!("{{ \"CO2\": {} }}", val).into()
            })
            .create();

        let mut sen = Sensor::new(name.to_string(), server.host_with_port());

        sen.client.retry_wait_time = Duration::from_secs(0);
        sen
    }

    fn input(state: StateRef, server: &mut Server, samples: Vec<u64>) -> Input {
        let metrics = metrics();
        let logger = logger();
        let mut input = Input::new(state, logger, metrics);

        input.sample_interval = Duration::from_secs(0);
        input.reduce_wait_time = Duration::from_secs(0);
        input.add_sensor(sensor(server, "office", samples));
        input
    }

    fn run(state: &mut State, input: &mut Input) {
        input.run_iteration();
        state.receive(Duration::from_secs(0));
    }

    #[test]
    fn test_applying_default_ventilation_in_response_to_co2_levels() {
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input =
            input(state_ref, &mut server, vec![450; SAMPLES as usize]);

        run(&mut state, &mut input);
        assert_eq!(state.rooms.get("office").unwrap().status, Status::Default);
    }

    #[test]
    fn test_applying_ventilation_in_response_to_different_co2_levels() {
        let tests = [
            (500_u64, Status::Default),
            (600, Status::Default),
            (625, Status::Default),
            (650, Status::Default),
            (675, Status::Default),
            (700, Status::Low),
            (725, Status::Low),
            (750, Status::Medium),
            (800, Status::MediumHigh),
            (825, Status::MediumHigh),
            (850, Status::MediumHigh),
            (875, Status::MediumHigh),
            (900, Status::High),
            (1000, Status::Maximum),
            (850, Status::MediumHigh),
            (800, Status::MediumHigh),
            (750, Status::MediumHigh),
            (700, Status::Low),
            (650, Status::Low),
            (625, Status::Default),
            (600, Status::Default),
        ];
        let samples =
            tests.iter().fold(Vec::new(), |mut samples, (level, _)| {
                samples.append(&mut vec![*level; SAMPLES as usize]);
                samples
            });
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input = input(state_ref, &mut server, samples);

        for (_, status) in tests {
            run(&mut state, &mut input);
            assert_eq!(state.rooms.get("office").unwrap().status, status);
        }
    }

    #[test]
    fn test_enabling_maximum_ventilation_in_response_to_co2_levels() {
        let samples = vec![
            875, 875, 875, 875, 875, 875, 875, 875, 875, 875, 875, 875, 875,
            875, 875, 875, 875, 875, 875, 875, 875, 875, 875, 875, 875, 875,
            875, 875, 875, 875, 900, 900, 900, 900, 900, 900, 900, 900, 900,
            900, 900, 900, 900, 900, 900, 900, 900, 900, 900, 900, 900, 900,
            900, 900, 900, 900, 900, 900, 900, 900,
        ];
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input = input(state_ref, &mut server, samples);

        run(&mut state, &mut input);
        run(&mut state, &mut input);
        assert_eq!(state.rooms.get("office").unwrap().status, Status::High);
    }

    #[test]
    fn test_ignoring_rooms_that_are_humid() {
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input =
            input(state_ref, &mut server, vec![950; SAMPLES as usize]);

        state.rooms.get_mut("office").unwrap().update(Status::Humid);
        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Humid);
    }

    #[test]
    fn test_ignoring_rooms_that_are_ventilated_in_response_to_a_button() {
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input =
            input(state_ref, &mut server, vec![950; SAMPLES as usize]);

        state
            .rooms
            .get_mut("office")
            .unwrap()
            .update(Status::Button(Box::new(Status::Default)));
        run(&mut state, &mut input);
        assert_eq!(
            state.rooms["office"].status,
            Status::Button(Box::new(Status::Default))
        );
    }

    #[test]
    fn test_maintaining_ventilation_for_a_while_when_co2_decreases() {
        let samples = vec![
            // The first update.
            750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750,
            750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750,
            750, 750, 750, 750,
            // The second update, where we'll maintain the speed for a while.
            650, 650, 650, 650, 650, 650, 650, 650, 650, 650, 650, 650, 650,
            650, 650, 650, 650, 650, 650, 650, 650, 650, 650, 650, 650, 650,
            650, 650, 650, 650,
            // The third update, where we'll reduce the speed.
            550, 550, 550, 550, 550, 550, 550, 550, 550, 550, 550, 550, 550,
            550, 550, 550, 550, 550, 550, 550, 550, 550, 550, 550, 550, 550,
            550, 550, 550, 550,
        ];
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input = input(state_ref, &mut server, samples);

        run(&mut state, &mut input);
        run(&mut state, &mut input);
        state.rooms.get_mut("office").unwrap().update(Status::Medium);
        run(&mut state, &mut input);

        assert_eq!(state.rooms["office"].status, Status::Default);
    }

    #[test]
    fn test_maintaining_ventilation_when_reducing_co2_from_750_to_550() {
        let samples = vec![
            // The first update.
            750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750,
            750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750,
            750, 750, 750, 750,
            // The second update, the speed is maintained.
            700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700,
            700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700,
            700, 700, 700, 700,
            // The third update, the speed is reduced.
            550, 550, 550, 550, 550, 550, 550, 550, 550, 550, 550, 550, 550,
            550, 550, 550, 550, 550, 550, 550, 550, 550, 550, 550, 550, 550,
            550, 550, 550, 550,
        ];
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input = input(state_ref, &mut server, samples);

        run(&mut state, &mut input);
        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Medium);

        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Default);
    }

    #[test]
    fn test_reducing_ventilation_immediately_when_reducing_co2_to_600() {
        let samples = vec![
            // The first update.
            700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700,
            700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700,
            700, 700, 700, 700,
            // The second update, the speed is reduced.
            600, 600, 600, 600, 600, 600, 600, 600, 600, 600, 600, 600, 600,
            600, 600, 600, 600, 600, 600, 600, 600, 600, 600, 600, 600, 600,
            600, 600, 600, 600,
        ];
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input = input(state_ref, &mut server, samples);

        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Low);

        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Default);
    }

    #[test]
    fn test_reducing_ventilation_immediately_when_reducing_co2_by_more_than_200()
     {
        let samples = vec![
            // The first update.
            900, 900, 900, 900, 900, 900, 900, 900, 900, 900, 900, 900, 900,
            900, 900, 900, 900, 900, 900, 900, 900, 900, 900, 900, 900, 900,
            900, 900, 900, 900,
            // The second update, the speed is lowered.
            700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700,
            700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700, 700,
            700, 700, 700, 700,
            // The third update, the speed is lowered again.
            500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500,
            500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500,
            500, 500, 500, 500,
        ];
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input = input(state_ref, &mut server, samples);

        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::High);

        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Low);

        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Default);
    }

    #[test]
    fn test_reducing_ventilation_immediately_when_reducing_co2_is_below_700() {
        let samples = vec![
            // The first update.
            750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750,
            750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750, 750,
            750, 750, 750, 750,
            // The second update, the speed is reduced
            675, 675, 675, 675, 675, 675, 675, 675, 675, 675, 675, 675, 675,
            675, 675, 675, 675, 675, 675, 675, 675, 675, 675, 675, 675, 675,
            675, 675, 675, 675,
        ];
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input = input(state_ref, &mut server, samples);

        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Medium);

        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Low);
    }

    #[test]
    fn test_using_a_custom_co2_minimum_threshold() {
        let samples = vec![
            400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400,
            400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400,
            400, 400, 400, 400,
        ];
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input = input(state_ref, &mut server, samples);

        input.sensors[0].co2_minimum = 400;

        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Low);
    }
}
