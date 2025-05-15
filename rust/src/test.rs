use crate::config::{
    Flow as FlowConfig, Itho as IthoConfig, Room as RoomConfig,
};
use crate::itho::Itho;
use crate::logger::{Level, Logger};
use crate::metrics::Metrics;
use crate::state::{Room, State, StateRef};
use mockito::Server;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

pub(crate) fn metrics() -> Arc<Metrics> {
    Metrics::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0).unwrap()
}

pub(crate) fn logger() -> Logger {
    let mut logger = Logger::new();

    logger.level = Level::None;
    logger
}

pub(crate) fn state(server: &mut Server) -> (State, StateRef) {
    let rooms = [
        RoomConfig {
            name: "living_room".to_string(),
            exhaust: 10,
            flow: FlowConfig {
                correction: 1.0,
                minimum: 0,
                maximum: 70,
                default: 0,
                low: 40,
                medium: 50,
                medium_high: 65,
                high: 75,
                button: 50,
            },
            motion: None,
            humidity: None,
            co2: None,
            co2_minimum: None,
        },
        RoomConfig {
            name: "office".to_string(),
            exhaust: 11,
            flow: FlowConfig {
                correction: 1.1,
                minimum: 10,
                maximum: 80,
                default: 20,
                low: 5,
                medium: 50,
                medium_high: 65,
                high: 90,
                button: 80,
            },
            motion: None,
            humidity: None,
            co2: None,
            co2_minimum: None,
        },
        RoomConfig {
            name: "super_fast".to_string(),
            exhaust: 15,
            flow: FlowConfig {
                correction: 1.0,
                minimum: 0,
                maximum: 400,
                default: 0,
                low: 40,
                medium: 50,
                medium_high: 65,
                high: 75,
                button: 400,
            },
            motion: None,
            humidity: None,
            co2: None,
            co2_minimum: None,
        },
        RoomConfig {
            name: "bathroom".to_string(),
            exhaust: 15,
            flow: FlowConfig {
                correction: 1.0,
                minimum: 0,
                maximum: 120,
                default: 10,
                low: 40,
                medium: 50,
                medium_high: 65,
                high: 75,
                button: 120,
            },
            motion: None,
            humidity: Some("RH bathroom 1".to_string()),
            co2: None,
            co2_minimum: None,
        },
    ];

    state_with_rooms(server, &rooms)
}

pub(crate) fn state_with_rooms(
    server: &mut Server,
    rooms: &[RoomConfig],
) -> (State, StateRef) {
    let metrics = metrics();
    let mut itho = Itho::new(server.host_with_port());

    itho.retry_wait_time = Duration::from_secs(0);

    let logger = logger();
    let conf = IthoConfig {
        address: server.host_with_port(),
        minimum_flow: 75,
        maximum_flow: 350,
        speed_id: 124,
        manual_id: 111,
        adjust_time: 0,
    };

    let (mut state, state_ref) = State::new(itho, logger, metrics, &conf);

    for room in rooms {
        state.add_room(Room::new(room.name.clone(), room));
    }

    (state, state_ref)
}
