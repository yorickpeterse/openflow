use crate::http::retry;
use jzon::{Error as JsonError, parse as parse_json};
use std::collections::HashMap;
use std::fmt;
use std::time::Duration;
use ureq::{Agent, Error as HttpError};

/// The time after which a request times out, excluding retries.
const TIMEOUT: u64 = 60;

/// The time (in seconds) to wait between retries.
const RETRY_TIME: u64 = 10;

/// The status of an RF remote.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) enum RemoteStatus {
    Unknown,
    Low,
    High,
    Cook30,
    Cook60,
    Timer1,
    Timer2,
    Timer3,
}

impl From<u64> for RemoteStatus {
    fn from(value: u64) -> Self {
        match value {
            6 => Self::High,
            8 => Self::Timer1,
            9 => Self::Timer2,
            10 => Self::Timer3,
            13 => Self::Cook30,
            14 => Self::Cook60,
            _ => Self::Low,
        }
    }
}

impl RemoteStatus {
    pub(crate) fn name(self) -> &'static str {
        match self {
            RemoteStatus::Unknown => "unknown",
            RemoteStatus::Low => "low",
            RemoteStatus::High => "high",
            RemoteStatus::Cook30 => "cook30",
            RemoteStatus::Cook60 => "cook60",
            RemoteStatus::Timer1 => "timer1",
            RemoteStatus::Timer2 => "timer2",
            RemoteStatus::Timer3 => "timer3",
        }
    }
}

#[derive(Copy, Clone)]
pub(crate) struct Remote {
    /// The current status of the remote.
    pub(crate) status: RemoteStatus,

    /// The time at which the command was received.
    pub(crate) timestamp: i64,
}

/// A type containing various statistics reported by the Itho WiFi module, such
/// as the CO2 levels and the humidity.
pub(crate) struct Status {
    /// The speed of the exhaust fan, as a percentage between 0 and 100.
    pub(crate) exhaust_speed: u64,

    /// The humidity levels for every humidity sensor.
    pub(crate) humidity: HashMap<String, u64>,
}

/// An error produced when retrieving a setting.
#[derive(Eq, PartialEq, Debug)]
pub(crate) enum Error {
    RequestFailed(String),
    InvalidRootValue,
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

/// A client for the Itho WiFi module API.
pub(crate) struct Itho {
    /// The hostname of the WiFi module.
    host: String,

    /// The HTTP client to use.
    http: Agent,

    /// The amount of time to wait after retrying a failed operation.
    pub(crate) retry_wait_time: Duration,
}

impl Itho {
    pub(crate) fn new(host: String) -> Self {
        let http = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(TIMEOUT)))
            .user_agent(format!("openflow {}", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();

        Self { host, http, retry_wait_time: Duration::from_secs(RETRY_TIME) }
    }

    /// Sets a setting.
    ///
    /// The `setting` argument must be the setting index as obtained from the
    /// "Itho settings" page. The `value` argument is the value for said
    /// setting. Only integer values are supported.
    pub(crate) fn set(&self, setting: u64, value: u64) -> Result<(), Error> {
        let url = self.url();
        let set = setting.to_string();
        let val = value.to_string();

        retry(self.retry_wait_time, move || {
            self.http
                .get(&url)
                .query("setsetting", &set)
                .query("value", &val)
                .call()
        })
        .map(|_| ())
        .map_err(|e| e.into())
    }

    pub(crate) fn get(&self, setting: u64) -> Result<u64, Error> {
        let url = self.url();
        let set = setting.to_string();
        let mut res = retry(self.retry_wait_time, move || {
            self.http.get(&url).query("getsetting", &set).call()
        })?;
        let body = res.body_mut().read_to_string()?;
        let json = parse_json(&body)?;

        Ok(json["current"].as_f64().unwrap_or(0.0) as u64)
    }

    pub(crate) fn status(&self) -> Result<Status, Error> {
        let url = self.url();
        let mut res = retry(self.retry_wait_time, move || {
            self.http.get(&url).query("get", "ithostatus").call()
        })?;
        let body = res.body_mut().read_to_string()?;
        let json = parse_json(&body)?;
        let obj = match json.as_object() {
            Some(o) => o,
            _ => return Err(Error::InvalidRootValue),
        };
        let speed = obj["exhaust fan (%)"].as_f64().unwrap_or(0.0) as u64;
        let mut map = HashMap::new();

        for (key, val) in obj.iter() {
            if !key.starts_with("RH ") {
                continue;
            }

            match val.as_f64().map(|n| n as u64) {
                // Sensors that aren't connected report a value that's very
                // close to zero, which due to the conversion to an Int is
                // turned into zero.
                Some(n) if n > 0 => {
                    map.insert(key.to_string(), n);
                }
                _ => {}
            }
        }

        Ok(Status { exhaust_speed: speed, humidity: map })
    }

    pub(crate) fn remotes(&self) -> Result<HashMap<String, Remote>, Error> {
        let url = self.url();
        let mut res = retry(self.retry_wait_time, move || {
            self.http.get(&url).query("get", "remotesinfo").call()
        })?;
        let body = res.body_mut().read_to_string()?;
        let json = parse_json(&body)?;
        let obj = match json.as_object() {
            Some(o) => o,
            _ => return Err(Error::InvalidRootValue),
        };

        let mut map = HashMap::new();

        for (name, val) in obj.iter() {
            let Some(val) = val.as_object() else { continue };
            let ts = val["timestamp"].as_f64().unwrap_or(0.0) as i64;
            let status = RemoteStatus::from(
                val["lastcmd"].as_f64().unwrap_or(0.0) as u64,
            );

            map.insert(name.to_string(), Remote { status, timestamp: ts });
        }

        Ok(map)
    }

    fn url(&self) -> String {
        format!("http://{}/api.html", self.host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    fn client(host: String) -> Itho {
        let mut client = Itho::new(host);

        client.retry_wait_time = Duration::from_secs(0);
        client
    }

    #[test]
    fn test_remote_status_name() {
        assert_eq!(RemoteStatus::Unknown.name(), "unknown");
        assert_eq!(RemoteStatus::Low.name(), "low");
        assert_eq!(RemoteStatus::High.name(), "high");
        assert_eq!(RemoteStatus::Cook30.name(), "cook30");
        assert_eq!(RemoteStatus::Cook60.name(), "cook60");
        assert_eq!(RemoteStatus::Timer1.name(), "timer1");
        assert_eq!(RemoteStatus::Timer2.name(), "timer2");
        assert_eq!(RemoteStatus::Timer3.name(), "timer3");
    }

    #[test]
    fn test_remote_status_eq() {
        assert_eq!(RemoteStatus::Unknown, RemoteStatus::Unknown);
        assert_eq!(RemoteStatus::Low, RemoteStatus::Low);
        assert_eq!(RemoteStatus::High, RemoteStatus::High);
        assert_eq!(RemoteStatus::Cook30, RemoteStatus::Cook30);
        assert_eq!(RemoteStatus::Cook60, RemoteStatus::Cook60);
        assert_eq!(RemoteStatus::Timer1, RemoteStatus::Timer1);
        assert_eq!(RemoteStatus::Timer2, RemoteStatus::Timer2);
        assert_eq!(RemoteStatus::Timer3, RemoteStatus::Timer3);
        assert_ne!(RemoteStatus::Low, RemoteStatus::High);
    }

    #[test]
    fn test_remote_status_from_u64() {
        assert_eq!(RemoteStatus::from(6), RemoteStatus::High);
        assert_eq!(RemoteStatus::from(8), RemoteStatus::Timer1);
        assert_eq!(RemoteStatus::from(9), RemoteStatus::Timer2);
        assert_eq!(RemoteStatus::from(10), RemoteStatus::Timer3);
        assert_eq!(RemoteStatus::from(13), RemoteStatus::Cook30);
        assert_eq!(RemoteStatus::from(14), RemoteStatus::Cook60);
        assert_eq!(RemoteStatus::from(15), RemoteStatus::Low);
    }

    #[test]
    fn test_itho_set_with_ok_response() {
        let mut server = Server::new();
        let itho = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api.html?setsetting=10&value=100")
            .with_status(200)
            .create();

        assert_eq!(itho.set(10, 100), Ok(()));
        mock.assert();
    }

    #[test]
    fn test_itho_set_with_error_response() {
        let mut server = Server::new();
        let itho = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api.html?setsetting=10&value=100")
            .with_status(500)
            .expect(10)
            .create();

        assert!(matches!(itho.set(10, 100), Err(Error::RequestFailed(_))));
        mock.assert();
    }

    #[test]
    fn test_itho_get_with_int_response() {
        let mut server = Server::new();
        let itho = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api.html?getsetting=10")
            .with_status(200)
            .with_body("{ \"current\": 42 }")
            .create();

        assert_eq!(itho.get(10), Ok(42));
        mock.assert();
    }

    #[test]
    fn test_itho_get_with_float_response() {
        let mut server = Server::new();
        let itho = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api.html?getsetting=10")
            .with_status(200)
            .with_body("{ \"current\": 42.3 }")
            .create();

        assert_eq!(itho.get(10), Ok(42));
        mock.assert();
    }

    #[test]
    fn test_itho_get_with_error_response() {
        let mut server = Server::new();
        let itho = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api.html?getsetting=10")
            .with_status(500)
            .expect(10)
            .create();

        assert!(matches!(itho.get(10), Err(Error::RequestFailed(_))));
        mock.assert();
    }

    #[test]
    fn test_itho_status_with_int_response() -> Result<(), Error> {
        let mut server = Server::new();
        let itho = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api.html?get=ithostatus")
            .with_status(200)
            .with_body("{ \"exhaust fan (%)\": 42, \"RH bathroom1 (%)\": 50 }")
            .create();

        let status = itho.status()?;

        assert_eq!(status.exhaust_speed, 42);
        assert_eq!(status.humidity.get("RH bathroom1 (%)"), Some(&50));
        mock.assert();
        Ok(())
    }

    #[test]
    fn test_itho_status_with_float_response() -> Result<(), Error> {
        let mut server = Server::new();
        let itho = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api.html?get=ithostatus")
            .with_status(200)
            .with_body(
                "{ \"exhaust fan (%)\": 42.5, \"RH bathroom1 (%)\": 50.5 }",
            )
            .create();

        let status = itho.status()?;

        assert_eq!(status.exhaust_speed, 42);
        assert_eq!(status.humidity.get("RH bathroom1 (%)"), Some(&50));
        mock.assert();
        Ok(())
    }

    #[test]
    fn test_itho_status_with_invalid_json() {
        let mut server = Server::new();
        let itho = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api.html?get=ithostatus")
            .with_status(200)
            .with_body("42")
            .create();

        assert!(matches!(itho.status(), Err(Error::InvalidRootValue)));
        mock.assert();
    }

    #[test]
    fn test_itho_remotes_with_int_response() -> Result<(), Error> {
        let mut server = Server::new();
        let itho = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api.html?get=remotesinfo")
            .with_status(200)
            .with_body(
                "{ \"kitchen\": { \"timestamp\": 123, \"lastcmd\": 6 } }",
            )
            .create();

        let remotes = itho.remotes()?;

        assert!(matches!(
            remotes.get("kitchen"),
            Some(Remote { status: RemoteStatus::High, timestamp: 123 })
        ));

        mock.assert();
        Ok(())
    }

    #[test]
    fn test_itho_remotes_with_float_response() -> Result<(), Error> {
        let mut server = Server::new();
        let itho = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api.html?get=remotesinfo")
            .with_status(200)
            .with_body(
                "{ \"kitchen\": { \"timestamp\": 123.5, \"lastcmd\": 6.0 } }",
            )
            .create();

        let remotes = itho.remotes()?;

        assert!(matches!(
            remotes.get("kitchen"),
            Some(Remote { status: RemoteStatus::High, timestamp: 123 })
        ));

        mock.assert();
        Ok(())
    }

    #[test]
    fn test_itho_remotes_with_invalid_response() {
        let mut server = Server::new();
        let itho = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api.html?get=remotesinfo")
            .with_status(500)
            .expect(10)
            .create();

        assert!(matches!(itho.remotes(), Err(Error::RequestFailed(_))));
        mock.assert();
    }

    #[test]
    fn test_itho_remotes_with_invalid_json() {
        let mut server = Server::new();
        let itho = client(server.host_with_port());
        let mock = server
            .mock("GET", "/api.html?get=remotesinfo")
            .with_status(200)
            .with_body("42")
            .create();

        assert!(matches!(itho.remotes(), Err(Error::InvalidRootValue)));
        mock.assert();
    }
}
