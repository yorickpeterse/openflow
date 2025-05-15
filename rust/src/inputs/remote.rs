use crate::itho::{Itho, Remote, RemoteStatus};
use crate::logger::Logger;
use crate::state::{StateRef, Status};
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

/// The interval (in seconds) at which to check the remote status.
const INTERVAL: u64 = 15;

pub(crate) struct Button {
    rooms: Vec<String>,
    duration: Duration,
}

impl Button {
    pub(crate) fn new(rooms: Vec<String>, duration: Duration) -> Self {
        Self { rooms, duration }
    }
}

/// An input monitoring the state of an RF button and adjusting ventilation
/// accordingly.
pub(crate) struct Input {
    state: StateRef,
    logger: Logger,
    itho: Itho,

    /// The status of the RF remote.
    status: RemoteStatus,

    /// The interval at which to check the button state.
    interval: Duration,

    /// The timestamp of when the last command was received.
    timestamp: i64,

    /// The time after which the remote should be reset to its initial state.
    deadline: Instant,

    /// The ID of the remote as used by the Itho WiFi API.
    id: String,

    /// The button states and the corresponding ventilation rules.
    buttons: HashMap<String, Button>,
}

impl Input {
    pub(crate) fn new(
        state: StateRef,
        logger: Logger,
        itho: Itho,
        id: String,
    ) -> Self {
        Input {
            state,
            logger,
            itho,
            status: RemoteStatus::Low,
            interval: Duration::from_secs(INTERVAL),
            timestamp: 0,
            deadline: Instant::now(),
            id: id,
            buttons: HashMap::new(),
        }
    }

    pub(crate) fn add_button(&mut self, name: String, button: Button) {
        self.buttons.insert(name, button);
    }

    pub(crate) fn run(mut self) {
        thread::Builder::new()
            .name("remote input".to_string())
            .spawn(move || {
                loop {
                    self.run_iteration();
                    thread::sleep(self.interval);
                }
            })
            .unwrap();
    }

    fn run_iteration(&mut self) {
        match self.itho.remotes().map(|mut r| r.remove(&self.id)) {
            Ok(Some(v)) => self.check(v),
            Ok(_) => {
                self.logger
                    .error("no state is found for the Itho remote".to_string());
            }
            Err(e) => {
                self.logger.error(format!(
                    "failed to get the Itho remote status: {}",
                    e
                ));
            }
        }
    }

    fn check(&mut self, remote: Remote) {
        // When starting up we ignore the current button state. This way if you
        // restart say six hours after pressing the Cook30 button, we don't
        // start ventilating according to that button again.
        if self.timestamp == 0 {
            self.timestamp = remote.timestamp;
            return;
        }

        if remote.timestamp == self.timestamp {
            let reset = match self.status {
                RemoteStatus::Low => false,
                _ => (self.deadline - Instant::now()).is_zero(),
            };

            if reset {
                self.logger.info(format!(
                    "the {} button timer expired",
                    self.status.name()
                ));
                self.reset_rooms();
                self.status = RemoteStatus::Low;
            }

            return;
        }

        self.status = remote.status;
        self.timestamp = remote.timestamp;

        let (rooms, dur) = match self.status {
            RemoteStatus::Low => {
                self.logger.info(
                    "resetting the remote to its default state".to_string(),
                );
                self.reset_rooms();
                return;
            }
            state => {
                if let Some(btn) = self.buttons.get(state.name()) {
                    (btn.rooms.clone(), btn.duration)
                } else {
                    return;
                }
            }
        };

        self.enable(rooms);
        self.deadline = Instant::now() + dur;
    }

    fn enable(&mut self, enable: Vec<String>) {
        self.state.update(move |state| {
            state.reset_buttons();

            for name in enable {
                let room = state.rooms.get_mut(&name).unwrap();

                room.update(Status::Button(Box::new(room.status.clone())));
            }
        });
    }

    fn reset_rooms(&mut self) {
        self.state.update(|state| state.reset_buttons());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;
    use crate::test::{logger, state};
    use mockito::Server;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Copy, Clone)]
    enum RemoteState {
        Low(i64),
        Timer1(i64),
    }

    impl RemoteState {
        fn timestamp(self) -> i64 {
            match self {
                Self::Low(v) | Self::Timer1(v) => v,
            }
        }

        fn to_int(self) -> i64 {
            match self {
                Self::Low(_) => 0,
                Self::Timer1(_) => 8,
            }
        }
    }

    fn input(
        state: StateRef,
        server: &mut Server,
        states: Vec<RemoteState>,
    ) -> Input {
        let idx = AtomicUsize::new(0);

        server
            .mock("GET", "/api.html?get=remotesinfo")
            .with_status(200)
            .with_body_from_request(move |_| {
                let old = idx.fetch_add(1, Ordering::Relaxed);
                let val = states.get(old).cloned().unwrap();

                format!(
                    "{{ \"office\": {{ \"timestamp\": {}, \"lastcmd\": {} }} }}",
                    val.timestamp(),
                    val.to_int()
                )
                .into()
            })
            .create();

        let logger = logger();
        let itho = Itho::new(server.host_with_port());
        let name = "office".to_string();
        let mut input = Input::new(state, logger, itho, name.clone());
        let btn = Button::new(vec![name], Duration::from_secs(0));

        input.add_button("timer1".to_string(), btn);
        input
    }

    fn run(state: &mut State, input: &mut Input) {
        input.run_iteration();
        state.receive(Duration::from_secs(0));
    }

    #[test]
    fn test_the_initial_remote_state_is_ignored() {
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input =
            input(state_ref, &mut server, vec![RemoteState::Timer1(123)]);

        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Default);
    }

    #[test]
    fn test_the_state_is_ignored_if_the_timestamp_remains_the_same() {
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input = input(
            state_ref,
            &mut server,
            vec![RemoteState::Timer1(123), RemoteState::Timer1(123)],
        );

        run(&mut state, &mut input);
        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Default);
    }

    #[test]
    fn test_enabling_ventilation_in_response_to_a_button_press() {
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input = input(
            state_ref,
            &mut server,
            vec![RemoteState::Timer1(123), RemoteState::Timer1(456)],
        );

        run(&mut state, &mut input);
        run(&mut state, &mut input);
        assert_eq!(
            state.rooms["office"].status,
            Status::Button(Box::new(Status::Default))
        );
    }

    #[test]
    fn test_disabling_ventilation_when_a_button_times_out() {
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input = input(
            state_ref,
            &mut server,
            vec![
                RemoteState::Timer1(123),
                RemoteState::Timer1(456),
                RemoteState::Timer1(456),
            ],
        );

        run(&mut state, &mut input);
        run(&mut state, &mut input);
        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Default);
    }

    #[test]
    fn test_resetting_the_remote_in_response_to_the_low_button() {
        let mut server = Server::new();
        let (mut state, state_ref) = state(&mut server);
        let mut input = input(
            state_ref,
            &mut server,
            vec![
                RemoteState::Timer1(123),
                RemoteState::Timer1(456),
                RemoteState::Low(789),
            ],
        );

        run(&mut state, &mut input);
        run(&mut state, &mut input);
        run(&mut state, &mut input);
        assert_eq!(state.rooms["office"].status, Status::Default);
    }
}
