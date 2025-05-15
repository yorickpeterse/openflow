use crate::http::retry;
use jzon::{Error as JsonError, parse as parse_json};
use std::fmt;
use std::time::Duration;
use ureq::{Agent, Error as HttpError};

/// The time after which a request times out, excluding retries.
const TIMEOUT: u64 = 60;

/// The time (in seconds) to wait between retries.
const RETRY_TIME: u64 = 10;

#[derive(Eq, PartialEq, Debug)]
pub(crate) enum Error {
    RequestFailed(String),
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
            Error::InvalidJson(e) => write!(f, "the JSON is invalid: {}", e),
        }
    }
}

/// An API/HTTP client for interacting with MoreSense (https://moresense-nl.com/)
/// CO2 sensors.
pub(crate) struct MoreSense {
    /// The hostname of the sensor.
    host: String,

    /// The HTTP client to use.
    http: Agent,

    /// The amount of time to wait after retrying a failed operation.
    pub(crate) retry_wait_time: Duration,
}

impl MoreSense {
    pub(crate) fn new(host: String) -> Self {
        let http = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(TIMEOUT)))
            .user_agent(format!("openflow {}", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();

        Self { host, http, retry_wait_time: Duration::from_secs(RETRY_TIME) }
    }

    /// Returns the current CO2 value.
    pub(crate) fn co2(&self) -> Result<u64, Error> {
        let url = self.url();
        let mut res =
            retry(self.retry_wait_time, move || self.http.get(&url).call())?;
        let body = res.body_mut().read_to_string()?;
        let json = parse_json(&body)?;

        match json["CO2"].as_u64() {
            Some(v) => Ok(v),
            _ => Err(Error::InvalidJson(
                "the value of the \"CO2\" key must be an integer".to_string(),
            )),
        }
    }

    fn url(&self) -> String {
        format!("http://{}/VALUES", self.host)
    }
}
