use crate::http::retry;
use jzon::{Error as JsonError, parse as parse_json};
use std::collections::HashMap;
use std::fmt;
use std::time::Duration;
use ureq::{Agent, Error as HttpError};

/// The time (in seconds) after which a request times out.
const TIMEOUT: u64 = 60;

/// The time (in seconds) to wait between retries.
const RETRY_TIME: u64 = 10;

/// The sensor states.
pub(crate) struct Sensors {
    /// A mapping of sensor IDs to a boolean indicating if motion is detected.
    motion: HashMap<u64, bool>,
}

/// An error produced when talking to the Hue bridge.
#[derive(Eq, PartialEq, Debug)]
pub(crate) enum Error {
    /// The HTTP request failed somehow.
    RequestFailed(String),

    /// The root JSON value is invalid.
    InvalidRootValue,

    /// The JSON response is invalid.
    InvalidJson(String),
}

impl From<HttpError> for Error {
    fn from(value: HttpError) -> Self {
        Error::RequestFailed(value.to_string())
    }
}

impl From<JsonError> for Error {
    fn from(value: JsonError) -> Self {
        Error::InvalidJson(value.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::RequestFailed(e) => {
                write!(f, "the request failed: {}", e)
            }
            Error::InvalidRootValue => {
                write!(f, "the root JSON value must be an object")
            }
            Error::InvalidJson(e) => write!(f, "the JSON is invalid: {}", e),
        }
    }
}

/// An API/HTTP client for interacting with the Philips Hue bridge and motion
/// sensors.
pub(crate) struct Hue {
    /// The HTTP client to use.
    http: Agent,

    /// The host of the bridge.
    host: String,

    /// The name of the "user"/API token.
    user: String,

    /// The amount of time to wait after retrying a failed operation.
    retry_wait_time: Duration,
}

impl Hue {
    pub(crate) fn new(host: String, user: String) -> Self {
        let http = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(TIMEOUT)))
            .user_agent(format!("openflow {}", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();

        Self {
            http,
            host,
            user,
            retry_wait_time: Duration::from_secs(RETRY_TIME),
        }
    }

    pub(crate) fn sensors(&self) -> Result<Sensors, Error> {
        let url = self.url();
        let mut res =
            retry(self.retry_wait_time, move || self.http.get(&url).call())?;
        let body = res.body_mut().read_to_string()?;
        let json = parse_json(&body)?;
        let obj = match json.as_object() {
            Some(o) => o,
            _ => return Err(Error::InvalidRootValue),
        };
        let mut sensors = Sensors { motion: HashMap::new() };

        for (key, val) in obj.iter() {
            let id = key.parse::<u64>().map_err(|_| {
                Error::InvalidJson("all keys must be integers".to_string())
            })?;

            if val["type"] != "ZLLPresence" {
                continue;
            }

            sensors.motion.insert(
                id,
                val["state"]["presence"].as_bool().unwrap_or(false),
            );
        }

        Ok(sensors)
    }

    fn url(&self) -> String {
        format!("http://{}/api/{}/sensors", self.host, self.user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    fn client(host: String) -> Hue {
        let mut client = Hue::new(host, "alice".to_string());

        client.retry_wait_time = Duration::from_secs(0);
        client
    }

    #[test]
    fn test_sensors_with_ok_response() -> Result<(), Error> {
        let mut server = Server::new();
        let hue = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api/alice/sensors")
            .with_status(200)
            .with_body(
                r#"
{
  "10": { "type": "ZLLPresence", "state": { "presence": true } },
  "20": { "type": "ZLLPresence", "state": { "presence": false } },
  "30": { "type": "ZLLPresence" },
  "40": { "type": "ZLLWhatever", "state": { "presence": true } }
}
"#,
            )
            .create();

        let sensors = hue.sensors()?;

        assert_eq!(sensors.motion.get(&10), Some(&true));
        assert_eq!(sensors.motion.get(&20), Some(&false));
        assert_eq!(sensors.motion.get(&30), Some(&false));
        assert_eq!(sensors.motion.get(&40), None);

        mock.assert();
        Ok(())
    }

    #[test]
    fn test_sensors_with_invalid_root() {
        let mut server = Server::new();
        let hue = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api/alice/sensors")
            .with_status(200)
            .with_body("42")
            .create();

        assert!(matches!(hue.sensors(), Err(Error::InvalidRootValue)));

        mock.assert();
    }

    #[test]
    fn test_sensors_with_invalid_json_syntax() {
        let mut server = Server::new();
        let hue = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api/alice/sensors")
            .with_status(200)
            .with_body(r#"{ "key": "#)
            .create();

        assert!(matches!(hue.sensors(), Err(Error::InvalidJson(_))));

        mock.assert();
    }

    #[test]
    fn test_sensors_with_invalid_json_keys() {
        let mut server = Server::new();
        let hue = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api/alice/sensors")
            .with_status(200)
            .with_body(r#" { "foo": { "type": "ZLLPresence" } } "#)
            .create();

        assert!(matches!(hue.sensors(), Err(Error::InvalidJson(_))));

        mock.assert();
    }

    #[test]
    fn test_sensors_with_invalid_response() {
        let mut server = Server::new();
        let hue = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api/alice/sensors")
            .with_status(500)
            .expect(10)
            .create();

        assert!(matches!(hue.sensors(), Err(Error::RequestFailed(_))));

        mock.assert();
    }
}
