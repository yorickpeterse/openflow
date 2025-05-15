pub(crate) mod co2;
pub(crate) mod config;
pub(crate) mod histogram;
pub(crate) mod http;
pub(crate) mod hue;
pub(crate) mod inputs;
pub(crate) mod itho;
pub(crate) mod logger;
pub(crate) mod metrics;
pub(crate) mod more_sense;
pub(crate) mod state;

#[cfg(test)]
pub(crate) mod test;

use crate::config::Config;
use crate::inputs::co2::{Input as Co2Input, Sensor as Co2Sensor};
use crate::inputs::remote::{Button, Input as RemoteInput};
use crate::itho::Itho;
use crate::logger::Logger;
use crate::metrics::Metrics;
use crate::state::{Message, Room, State};
use getopts::Options;
use std::env;
use std::process::exit;
use std::thread;
use std::time::Duration;

const DEFAULT_CONFIG: &str = "/etc/openflow.json";

const USAGE: &str = "Usage: openflow [OPTIONS]

A ventilation system built around Itho Daalderop's DemandFlow/QualityFlow
ventilation system.

Examples:

  openflow                       # Start using the default configuration file
  openflow --config config.json  # Start using a custom configuration file";

pub(crate) fn print_usage(options: &Options, brief: &str) {
    let out = options.usage_with_format(|opts| {
        format!(
            "{}\n\nOptions:\n\n{}",
            brief,
            opts.collect::<Vec<String>>().join("\n")
        )
    });

    println!("{}", out);
}

pub(crate) fn error(message: String) -> ! {
    eprintln!("error: {}", message);
    exit(1);
}

fn main() {
    let mut opts = Options::new();

    opts.optflag("h", "help", "Show this help message");
    opts.optopt(
        "c",
        "config",
        &format!("The configuration file to use (default: {})", DEFAULT_CONFIG),
        "PATH",
    );

    let args: Vec<String> = env::args().collect();
    let matches = match opts.parse(args) {
        Ok(v) => v,
        Err(e) => error(e.to_string()),
    };

    if matches.opt_present("h") {
        print_usage(&opts, USAGE);
        return;
    }

    let config = match Config::load("config.json") {
        Ok(v) => v,
        Err(e) => error(format!("failed to parse the configuration: {}", e)),
    };
    let logger = Logger::new();
    let metrics = match Metrics::new(config.metrics.ip, config.metrics.port) {
        Ok(v) => v,
        Err(e) => error(format!("failed to set up the metrics: {}", e)),
    };

    let (mut state, state_ref) = State::new(
        Itho::new(config.itho.address.clone()),
        logger.clone(),
        metrics.clone(),
        &config.itho,
    );

    let mut co2_input =
        Co2Input::new(state_ref.clone(), logger.clone(), metrics.clone());

    for conf in &config.rooms {
        match &conf.co2 {
            Some(addr) => {
                let mut sensor =
                    Co2Sensor::new(conf.name.clone(), addr.clone());

                if let Some(v) = conf.co2_minimum {
                    sensor.co2_minimum = v
                }

                co2_input.add_sensor(sensor);
            }
            _ => {}
        }

        match conf.motion {
            Some(id) => {}
            _ => {}
        }

        match &conf.humidity {
            Some(id) => {}
            _ => {}
        }

        state.add_room(Room::new(conf.name.clone(), conf));
    }

    match &config.remote {
        Some(remote) => {
            let state = state_ref.clone();
            let itho = Itho::new(config.itho.address.clone());
            let logger = logger.clone();
            let id = remote.id.clone();
            let mut input = RemoteInput::new(state, logger, itho, id);

            for cfg in &remote.buttons {
                let btn = Button::new(cfg.rooms.clone(), cfg.duration);

                input.add_button(cfg.name.clone(), btn);
            }

            input.run();
        }
        _ => {}
    }

    co2_input.run();
    state.run();
}
