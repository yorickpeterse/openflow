use jzon::object::Object;
use jzon::{Error as JsonError, parse as parse_json};
use std::fmt;
use std::fs::read_to_string;
use std::io::Error as IoError;
use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

/// An error produced while parsing a configuration file.
#[derive(Eq, PartialEq, Debug)]
pub(crate) enum Error {
    InvalidFile(String),
    InvalidSyntax(String),
    InvalidConfig,
}

impl From<IoError> for Error {
    fn from(value: IoError) -> Self {
        Error::InvalidFile(value.to_string())
    }
}

impl From<JsonError> for Error {
    fn from(value: JsonError) -> Self {
        Error::InvalidSyntax(value.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidFile(e) => e.fmt(f),
            Self::InvalidSyntax(e) => {
                write!(f, "the JSON syntax is invalid: {}", e)
            }
            Self::InvalidConfig => {
                write!(f, "the configuration is invalid")
            }
        }
    }
}

fn opt_int(object: &Object, key: &str) -> Option<i64> {
    object[key].as_i64()
}

fn opt_uint(object: &Object, key: &str) -> Option<u64> {
    object[key].as_u64()
}

fn opt_string(object: &Object, key: &str) -> Option<String> {
    object[key].as_str().map(|s| s.to_string())
}

fn string(object: &Object, key: &str) -> Result<String, Error> {
    object[key]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| Error::InvalidConfig)
}

fn uint(object: &Object, key: &str) -> Result<u64, Error> {
    object[key].as_u64().ok_or_else(|| Error::InvalidConfig)
}

fn int(object: &Object, key: &str) -> Result<i64, Error> {
    object[key].as_i64().ok_or_else(|| Error::InvalidConfig)
}

fn float(object: &Object, key: &str) -> Result<f64, Error> {
    object[key].as_f64().ok_or_else(|| Error::InvalidConfig)
}

fn opt_ip(object: &Object, key: &str) -> Result<Option<IpAddr>, Error> {
    match object[key].as_str() {
        Some(v) => {
            Ok(Some(v.parse::<IpAddr>().map_err(|_| Error::InvalidConfig)?))
        }
        _ => Ok(None),
    }
}

fn ip(object: &Object, key: &str) -> Result<IpAddr, Error> {
    if let Some(v) = object[key].as_str() {
        if let Ok(ip) = v.parse::<IpAddr>() {
            return Ok(ip);
        }
    }

    Err(Error::InvalidConfig)
}

/// Configuration details for the air flow of a room.
pub(crate) struct Flow {
    /// The value to multiply the flow by to obtain the true air flow.
    ///
    /// Given a configured air flow of X m3/h, the actual air flow may end up
    /// lower due to the distance from the exhaust to the ventilation system,
    /// the amount of corners the duct has to take, the material of the duct,
    /// etc.
    ///
    /// This value is multiplied with the flow to correct for such variables.
    /// Obtaining these values is done as follows:
    ///
    /// 1. Set the ventilation system to a fixed exhaust speed.
    /// 2. Close all exhausts, except the one you want to measure.
    /// 3. Using an anemometer, measure the amount of air flowing through the
    ///    exhaust (in m3/h).
    /// 4. Do this for all the exhausts.
    /// 5. Take the maximum value across the exhausts, then for each
    ///    exhaust/room derive this value using the formula `1 + (1 - (flow /
    ///    max))`, then round it up slightly (e.g. to the nearest multiple of
    ///    0.05) to account for measurement errors.
    pub(crate) correction: f64,

    /// The minimum air flow for active ventilation in m3/h.
    pub(crate) minimum: u64,

    /// The maximum air flow in m3/h.
    pub(crate) maximum: u64,

    /// The default air flow in m3/h.
    pub(crate) default: u64,

    /// The air flow in m3/h for the "low" setting.
    pub(crate) low: u64,

    /// The air flow in m3/h for the "medium" setting.
    pub(crate) medium: u64,

    /// The air flow in m3/h for the "medium high" setting.
    pub(crate) medium_high: u64,

    /// The air flow in m3/h for the "high" setting.
    pub(crate) high: u64,

    /// The air flow in m3/h to apply when enabling ventilation in response to a
    /// button.
    pub(crate) button: u64,
}

impl Flow {
    pub(crate) fn from_json(object: &Object) -> Result<Flow, Error> {
        let correct = float(object, "correction")?;
        let min = uint(object, "minimum")?;
        let max = uint(object, "maximum")?;
        let def = uint(object, "default")?;
        let low = uint(object, "low")?;
        let med = uint(object, "medium")?;
        let med_high = uint(object, "medium_high")?;
        let high = uint(object, "high")?;
        let button = opt_uint(object, "button").unwrap_or(max);

        Ok(Flow {
            correction: correct,
            minimum: min,
            maximum: max,
            default: def,
            low: low,
            medium: med,
            medium_high: med_high,
            high: high,
            button: button,
        })
    }
}

/// Configuration details for a single room.
pub(crate) struct Room {
    /// The unique name of the room.
    pub(crate) name: String,

    /// The setting ID of the exhaust for this room.
    pub(crate) exhaust: u64,

    /// The air flow configuration for this room.
    pub(crate) flow: Flow,

    /// The ID of the motion sensor associated with this room.
    pub(crate) motion: Option<u64>,

    /// The name of the humidity sensor associated with this room.
    pub(crate) humidity: Option<String>,

    /// The address of a MoreSense CO2 sensor associated with this room.
    pub(crate) co2: Option<String>,

    /// The minimum CO2 value to observe before enabling active ventilation.
    pub(crate) co2_minimum: Option<u64>,
}

impl Room {
    pub(crate) fn from_json(
        name: String,
        object: &Object,
    ) -> Result<Room, Error> {
        let exhaust = uint(object, "exhaust")?;
        let flow = match object["flow"].as_object() {
            Some(o) => Flow::from_json(o)?,
            _ => return Err(Error::InvalidConfig),
        };
        let motion = opt_uint(object, "motion");
        let humidity = opt_string(object, "humidity");
        let co2 = opt_string(object, "co2");
        let co2_min = opt_uint(object, "co2_minimum");

        Ok(Room {
            name: name,
            flow: flow,
            exhaust: exhaust,
            motion: motion,
            humidity: humidity,
            co2: co2,
            co2_minimum: co2_min,
        })
    }
}

/// Configuration details for the Itho ventilation unit.
pub(crate) struct Itho {
    /// The address of the Itho WiFi module.
    pub(crate) address: String,

    /// The ID of the setting that controls the manual mode.
    pub(crate) manual_id: u64,

    /// The ID of the setting that controls the ventilation speed.
    pub(crate) speed_id: u64,

    /// The minimum air flow to always apply.
    pub(crate) minimum_flow: u64,

    /// The maximum air flow supported by the unit.
    pub(crate) maximum_flow: u64,

    /// The amount of time to wait for the valves to adjust their setting.
    pub(crate) adjust_time: u64,
}

impl Itho {
    pub(crate) fn from_json(object: &Object) -> Result<Itho, Error> {
        let address = string(object, "address")?;
        let manual = uint(object, "manual_id")?;
        let speed = uint(object, "speed_id")?;
        let min = uint(object, "minimum_flow")?;
        let max = uint(object, "maximum_flow")?;
        let adjust_time = opt_uint(object, "adjust_time").unwrap_or(5);

        Ok(Itho {
            address,
            manual_id: manual,
            speed_id: speed,
            minimum_flow: min,
            maximum_flow: max,
            adjust_time: adjust_time,
        })
    }
}

/// Configuration details for the metrics database
pub(crate) struct Metrics {
    /// The IP address to connect to.
    pub(crate) ip: IpAddr,

    /// The port number of the server.
    pub(crate) port: u16,
}

impl Metrics {
    pub(crate) fn from_json(object: &Object) -> Result<Metrics, Error> {
        let ip = ip(object, "ip")?;
        let port = uint(object, "port")? as u16;

        Ok(Metrics { ip: ip, port: port })
    }
}

/// Configuration details for the Hue API
pub(crate) struct Hue {
    /// The IP address to connect to.
    ip: IpAddr,

    /// The user/API token to use.
    user: String,
}

impl Hue {
    pub(crate) fn from_json(object: &Object) -> Result<Hue, Error> {
        let ip = ip(object, "ip")?;
        let user = string(object, "user")?;

        Ok(Hue { ip: ip, user: user })
    }
}

/// Configuration settings for the humidity sensors.
pub(crate) struct Humidity {
    /// The threshold at which to start ventilating at maximum speed.
    high: u64,

    /// The threshold at which to return to normal ventilation.
    low: u64,

    /// If humidity increases by this value then ventilation is enabled,
    /// regardless of the absolute value.
    max_increase: u64,

    /// The value to add to the raw sensor values to obtain the correct value.
    ///
    /// The Itho humidity sensors appear to not be entirely accurate, sometimes
    /// reporting values 5-10% higher than reality. This value can be used to
    /// correct for such inaccuracies.
    correction: i64,
}

impl Humidity {
    pub(crate) fn from_json(object: &Object) -> Result<Humidity, Error> {
        let high = uint(object, "high")?;
        let low = uint(object, "low")?;
        let max_increase = opt_uint(object, "max_increase").unwrap_or(15);
        let correct = opt_int(object, "correction").unwrap_or(-5);

        Ok(Humidity {
            high: high,
            low: low,
            max_increase: max_increase,
            correction: correct,
        })
    }
}

/// Configuration details for a single button.
pub(crate) struct Button {
    /// The name of the button state.
    pub(crate) name: String,

    /// The rooms to ventilate.
    pub(crate) rooms: Vec<String>,

    /// The time to ventilate the room for.
    pub(crate) duration: Duration,
}

impl Button {
    pub(crate) fn from_json(
        name: String,
        object: &Object,
    ) -> Result<Button, Error> {
        let Some(vals) = object["rooms"].as_array() else {
            return Err(Error::InvalidConfig);
        };
        let rooms = vals.iter().try_fold(Vec::new(), |mut rooms, val| {
            let Some(v) = val.as_str() else {
                return Err(Error::InvalidConfig);
            };
            rooms.push(v.to_string());
            Ok(rooms)
        })?;

        let duration = Duration::from_secs(uint(object, "duration")?);

        Ok(Button { name: name, rooms: rooms, duration: duration })
    }
}

/// Configuration details for the RF remote.
pub(crate) struct Remote {
    /// The ID/name of the remote as used in the API.
    pub(crate) id: String,

    /// The button states and their rooms to ventilate.
    pub(crate) buttons: Vec<Button>,
}

impl Remote {
    pub(crate) fn from_json(object: &Object) -> Result<Remote, Error> {
        let id = string(object, "id")?;
        let mut buttons = Vec::new();

        if let Some(obj) = object["buttons"].as_object() {
            for (key, val) in obj.iter() {
                let Some(v) = val.as_object() else {
                    return Err(Error::InvalidConfig);
                };
                buttons.push(Button::from_json(key.to_string(), v)?);
            }
        } else {
            return Err(Error::InvalidConfig);
        };

        Ok(Remote { id: id, buttons: buttons })
    }
}

/// All configuration details.
pub(crate) struct Config {
    /// The configuration details for each room to ventilate.
    pub(crate) rooms: Vec<Room>,

    /// The configuration details for the Itho ventilation unit.
    pub(crate) itho: Itho,

    /// The configuration details for the metrics database.
    pub(crate) metrics: Metrics,

    /// The configuration details for the Hue API.
    pub(crate) hue: Hue,

    /// The configuration details for the humidity sensors.
    pub(crate) humidity: Humidity,

    /// The configuration details for the RF remote.
    pub(crate) remote: Option<Remote>,
}

impl Config {
    /// Load configuration from a JSON file.
    pub(crate) fn load<P: AsRef<Path>>(path: P) -> Result<Config, Error> {
        let data = read_to_string(path)?;
        let doc = parse_json(&data)?;
        let Some(root) = doc.as_object() else {
            return Err(Error::InvalidConfig);
        };
        let mut rooms = Vec::new();

        match root["rooms"].as_object() {
            Some(obj) => {
                for (key, val) in obj.iter() {
                    let name = key.to_string();

                    match val.as_object() {
                        Some(v) => rooms.push(Room::from_json(name, v)?),
                        _ => return Err(Error::InvalidConfig),
                    }
                }
            }
            _ => return Err(Error::InvalidConfig),
        };

        let itho = match root["itho"].as_object() {
            Some(v) => Itho::from_json(v)?,
            _ => return Err(Error::InvalidConfig),
        };

        let metrics = match root["metrics"].as_object() {
            Some(v) => Metrics::from_json(v)?,
            _ => return Err(Error::InvalidConfig),
        };

        let hue = match root["hue"].as_object() {
            Some(v) => Hue::from_json(v)?,
            _ => return Err(Error::InvalidConfig),
        };

        let humidity = match root["humidity"].as_object() {
            Some(v) => Humidity::from_json(v)?,
            _ => return Err(Error::InvalidConfig),
        };

        let remote = match root["remote"].as_object() {
            Some(v) => Some(Remote::from_json(v)?),
            _ => None,
        };

        Ok(Config {
            rooms: rooms,
            itho: itho,
            metrics: metrics,
            hue: hue,
            humidity: humidity,
            remote: remote,
        })
    }
}
