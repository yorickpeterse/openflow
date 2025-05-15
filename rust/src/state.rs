use crate::config::{Itho as IthoConfig, Room as RoomConfig};
use crate::itho::Itho;
use crate::logger::Logger;
use crate::metrics::Metrics;
use std::cmp::{Ord, Ordering, PartialOrd, max, min};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

/// The maximum value of an exhaust valve setting.
///
/// While the API/hardware limit is 5000, the valves can make a distinct bonking
/// noise when opening them this far. The actual Itho hardware also appears to
/// limit the setting to this value.
const EXHAUST_MAX: u64 = 4000;

/// The capacity of the State message mailbox.
///
/// While it's unlikely that we'll ever send _a lot_ of messages to the State
/// channel, it doesn't hurt to enforce an upper limit before senders are
/// blocked.
const STATE_MAILBOX_CAPACITY: usize = 32;

/// The time (in seconds) between hardware updates.
const APPLY_INTERVAL: u64 = 60;

fn exhaust_percentage(flow: u64, total_flow: u64) -> u64 {
    if flow >= total_flow {
        return EXHAUST_MAX;
    }

    if flow == 0 {
        return 0;
    }

    let raw = (((flow * 100) / total_flow) * EXHAUST_MAX) / 100;

    // To reduce the amount of micro adjustments of the exhaust motors, we round
    // the setting values up to the nearest multiple of 100, so 625 becomes 700.
    ((raw + 99) / 100) * 100
}

/// The ventilation status of a room.
#[derive(Clone, Eq, PartialEq, Debug)]
pub(crate) enum Status {
    /// The default state, optionally applying a minimum amount of ventilation
    /// based on the total ventilation need.
    Default,

    /// The room is ventilated at the low speed.
    Low,

    /// The room is ventilated at the medium speed.
    Medium,

    /// The room is ventilated at the medium high speed.
    MediumHigh,

    /// The room is ventilated at the high speed.
    High,

    /// Ventilation is running at the maximum speed.
    Maximum,

    /// Ventilation is enabled in response to an RF button.
    ///
    /// The wrapped value is the old status to transition back to.
    Button(Box<Status>),

    /// Ventilation is enabled in response to high humidity.
    Humid,
}

impl Status {
    pub(crate) fn is_humid(&self) -> bool {
        matches!(self, Status::Humid)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Status::Default => write!(f, "Default"),
            Status::Low => write!(f, "Low"),
            Status::Medium => write!(f, "Medium"),
            Status::MediumHigh => write!(f, "MediumHigh"),
            Status::High => write!(f, "High"),
            Status::Maximum => write!(f, "Maximum"),
            Status::Button(_) => write!(f, "Button"),
            Status::Humid => write!(f, "Humid"),
        }
    }
}

impl PartialOrd for Status {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Status {
    fn cmp(&self, other: &Self) -> Ordering {
        use Status::*;

        match (self, other) {
            (Default, Default) => Ordering::Equal,
            (Default, _) => Ordering::Less,
            (Low, Low) => Ordering::Equal,
            (Low, Default) => Ordering::Greater,
            (Low, _) => Ordering::Less,
            (Medium, Medium) => Ordering::Equal,
            (Medium, Default | Low) => Ordering::Greater,
            (Medium, _) => Ordering::Less,
            (MediumHigh, MediumHigh) => Ordering::Equal,
            (MediumHigh, Default | Low | Medium) => Ordering::Greater,
            (MediumHigh, _) => Ordering::Less,
            (High, High) => Ordering::Equal,
            (High, Default | Low | Medium | MediumHigh) => Ordering::Greater,
            (High, _) => Ordering::Less,
            (Maximum, Maximum) => Ordering::Equal,
            (Maximum, Button(_) | Humid) => Ordering::Less,
            (Maximum, _) => Ordering::Greater,
            (Button(a), Button(b)) => a.cmp(b),
            (Button(_), Humid | Maximum) => Ordering::Less,
            (Button(_), _) => Ordering::Greater,
            (Humid, Humid) => Ordering::Equal,
            (Humid, _) => Ordering::Greater,
        }
    }
}

/// The state of a single room.
pub(crate) struct Room {
    /// The unique ID/name of the room.
    name: String,

    /// The ventilation status of this room.
    pub(crate) status: Status,

    /// The last time the status was updated.
    last_update: Instant,

    /// The value to multiply the raw air flow by to account for pressure loss
    /// in the air duct.
    flow_correction: f64,

    /// The ID of the exhaust for this room.
    exhaust_id: u64,

    /// The exhaust setting value, in a range from 0 to 4000.
    ///
    /// This value defaults to `u64::MAX`. This ensures that the first time we
    /// make any changes, we don't ignore exhausts with a target value of zero,
    /// as that could result in them remaining in whatever state they were
    /// before we started.
    exhaust_value: u64,

    /// The default air flow in m3/hour, without the flow correction.
    default_flow: u64,

    /// The air flow in m3/h for the "low" setting.
    low_flow: u64,

    /// The air flow in m3/h for the "medium" setting.
    medium_flow: u64,

    /// The air flow in m3/h for the "medium high" setting.
    medium_high_flow: u64,

    /// The air flow in m3/h for the "high" setting.
    high_flow: u64,

    /// The minimum air flow in m3/hour, without the flow correction.
    minimum_flow: u64,

    /// The maximum air flow in m3/hour, without the flow correction.
    maximum_flow: u64,

    /// The button air flow in m3/hour, without the flow correction.
    button_flow: u64,

    /// The air flow in m3/hour, without the flow correction applied
    base_flow: u64,

    /// The air flow in m3/hour, with the flow correction applied
    current_flow: u64,
}

impl Room {
    pub(crate) fn new(name: String, config: &RoomConfig) -> Room {
        Room {
            name,
            status: Status::Default,
            flow_correction: config.flow.correction,
            exhaust_id: config.exhaust,
            exhaust_value: u64::MAX,
            default_flow: config.flow.default,
            low_flow: config.flow.low,
            medium_flow: config.flow.medium,
            medium_high_flow: config.flow.medium_high,
            high_flow: config.flow.high,
            minimum_flow: config.flow.minimum,
            maximum_flow: config.flow.maximum,
            button_flow: config.flow.button,
            base_flow: 0,
            current_flow: 0,
            last_update: Instant::now(),
        }
    }

    pub(crate) fn update(&mut self, status: Status) {
        self.status = status;
        self.last_update = Instant::now();
    }

    pub(crate) fn update_flow(&mut self, flow: u64, maximum: u64) {
        if flow == 0 {
            self.base_flow = min(self.minimum_flow, maximum);
            self.current_flow = self.correct(self.base_flow);
            return;
        }

        self.base_flow =
            min(min(max(flow, self.minimum_flow), self.maximum_flow), maximum);
        self.current_flow = self.correct(self.base_flow);
    }

    fn correct(&self, flow: u64) -> u64 {
        (flow as f64 * self.flow_correction).ceil() as u64
    }
}

/// A message to send to the State thread.
pub(crate) enum Message {
    /// Runs the supplied function in the context of the state.
    ///
    /// This allows applying of multiple updates in a single
    /// message/transaction.
    ///
    /// TODO: split into smaller messages
    Update(Box<dyn FnOnce(&mut State) + Send + 'static>),

    /// Resets the button state of each room.
    ResetButton,
}

pub(crate) struct State {
    /// The channel to receive messages on.
    messages: Receiver<Message>,

    /// The minimum global air flow.
    minimum_flow: u64,

    /// The maximum global air flow.
    maximum_flow: u64,

    /// The ID of the setting that controls the ventilation speed.
    speed_id: u64,

    /// The ID of the setting that enables/disables manual control of the
    /// ventilation unit.
    manual_id: u64,

    /// The speed (as a percentage of its maximum) the ventilation unit is
    /// running at.
    speed: u64,

    /// The rooms to ventilate along with their current state.
    pub(crate) rooms: HashMap<String, Room>,

    /// The amount of time to wait (in seconds) for exhaust valves to adjust
    /// themselves.
    adjust_time: Duration,

    logger: Logger,
    itho: Itho,
    metrics: Arc<Metrics>,
}

impl State {
    pub(crate) fn new(
        itho: Itho,
        logger: Logger,
        metrics: Arc<Metrics>,
        config: &IthoConfig,
    ) -> (Self, StateRef) {
        let (send, rec) = sync_channel(STATE_MAILBOX_CAPACITY);
        let state = Self {
            messages: rec,
            itho: itho,
            metrics: metrics,
            logger: logger,
            minimum_flow: config.minimum_flow,
            maximum_flow: config.maximum_flow,
            speed_id: config.speed_id,
            manual_id: config.manual_id,
            speed: 0,
            rooms: HashMap::new(),
            adjust_time: Duration::from_secs(config.adjust_time),
        };

        (state, StateRef { inner: send })
    }

    pub(crate) fn run(&mut self) {
        // Prepare and apply the initial state.
        self.prepare();
        self.apply();

        let mut next_apply =
            Instant::now() + Duration::from_secs(APPLY_INTERVAL);

        while self.receive(next_apply - Instant::now()) {
            if (next_apply - Instant::now()).is_zero() {
                self.apply();
                next_apply =
                    Instant::now() + Duration::from_secs(APPLY_INTERVAL);
            }
        }
    }

    pub(crate) fn receive(&mut self, timeout: Duration) -> bool {
        match self.messages.recv_timeout(timeout) {
            Ok(Message::Update(f)) => f(self),
            Ok(Message::ResetButton) => self.reset_buttons(),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return false,
        }

        true
    }

    pub(crate) fn prepare(&mut self) {
        let man = self.manual_id;

        if self.itho.get(man).unwrap_or(0) == 1 {
            return;
        }

        self.logger.info("enabling manual control".to_string());
        self.itho.set(man, 1).unwrap();
        thread::sleep(self.adjust_time * (self.rooms.len() as u32));
    }

    pub(crate) fn add_room(&mut self, room: Room) {
        self.rooms.insert(room.name.clone(), room);
    }

    pub(crate) fn reset_buttons(&mut self) {
        for room in self.rooms.values_mut() {
            if let Status::Button(old) = room.status.clone() {
                room.update(*old);
            }
        }
    }

    pub(crate) fn apply(&mut self) {
        let per_room = self.update_flow_per_room();
        let total_flow =
            max(self.minimum_flow, min(per_room, self.maximum_flow));
        let new_speed = self.flow_percentage(total_flow);

        if new_speed < self.speed {
            self.update_speed(new_speed);
        }

        for room in self.rooms.values_mut() {
            let flow = room.current_flow;
            let new = exhaust_percentage(flow, total_flow);
            let old = room.exhaust_value;

            self.metrics.add("air_flow", |m| {
                m.tag("room", &room.name);
                m.field("rate", flow);
            });

            if new == old {
                continue;
            }

            self.logger.info(format!(
                "{}: changing exhaust to {} ({}, {} m3/h)",
                room.name, new, room.status, flow
            ));

            self.itho
                .set(room.exhaust_id, new)
                .expect("the exhaust should be updated");
            room.exhaust_value = new;
            thread::sleep(self.adjust_time);
        }

        if new_speed > self.speed {
            self.update_speed(new_speed);
        }

        self.metrics.add("exhaust_speed", |m| {
            m.field("percentage", new_speed);
        });
    }

    fn update_speed(&mut self, speed: u64) {
        self.logger.info(format!(
            "changing exhaust speed from {}% to {}%",
            self.speed, speed
        ));
        self.itho
            .set(self.speed_id, speed)
            .expect("the exhaust speed should be updated");
        self.speed = speed;
        thread::sleep(self.adjust_time);
    }

    fn update_flow_per_room(&mut self) -> u64 {
        let humid =
            self.rooms.values().any(|r| matches!(r.status, Status::Humid));
        let total = self.rooms.values_mut().fold(0, |sum, room| {
            let base = if humid {
                match &room.status {
                    Status::Default => 0,
                    Status::Humid => room.maximum_flow,
                    _ => 20,
                }
            } else {
                match &room.status {
                    Status::Default => 0,
                    Status::Low => room.low_flow,
                    Status::Medium => room.medium_flow,
                    Status::MediumHigh => room.medium_high_flow,
                    Status::High => room.high_flow,
                    Status::Button(_) => room.button_flow,
                    Status::Maximum | Status::Humid => room.maximum_flow,
                }
            };

            room.update_flow(base, self.maximum_flow);
            sum + room.current_flow
        });

        if total >= self.minimum_flow {
            return total;
        }

        self.assign_default_flow(total);
        self.rooms.values().fold(0, |sum, room| sum + room.current_flow)
    }

    fn assign_default_flow(&mut self, total: u64) {
        let mut extra = self.minimum_flow - total;

        for room in self.rooms.values_mut() {
            match &room.status {
                Status::Default if room.default_flow > 0 => {}
                _ => continue,
            }

            if extra == 0 || (room.current_flow == 0 && extra < 10) {
                continue;
            }

            let add = max(min(room.default_flow - room.minimum_flow, extra), 0);
            let new = room.base_flow + add;

            extra -= add;
            room.update_flow(new, self.maximum_flow);
        }
    }

    fn flow_percentage(&mut self, flow: u64) -> u64 {
        flow * 100 / self.maximum_flow
    }
}

#[derive(Clone)]
pub(crate) struct StateRef {
    inner: SyncSender<Message>,
}

impl StateRef {
    /// Schedules the closure for execution within the state's context.
    ///
    /// The return value is a boolean indicating if the message was sent (true)
    /// or if the channel is closed (false).
    pub(crate) fn update<F: FnOnce(&mut State) + Send + 'static>(
        &self,
        func: F,
    ) -> bool {
        self.inner.send(Message::Update(Box::new(func))).is_ok()
    }
}
