use std::thread::sleep;
use std::time::Duration;
use ureq::http::Response;
use ureq::{Body, Error};

/// The amount of times we'll retry the operation.
const RETRIES: usize = 10;

pub(crate) fn retry<F: FnMut() -> Result<Response<Body>, Error>>(
    wait: Duration,
    mut func: F,
) -> Result<Response<Body>, Error> {
    let mut attempts = 1;

    loop {
        match func() {
            Ok(r) => return Ok(r),
            Err(Error::StatusCode(v)) if v == 501 => {
                return Err(Error::StatusCode(v));
            }
            Err(e) if attempts == RETRIES => return Err(e),
            _ => {
                attempts += 1;
                sleep(wait);
            }
        }
    }
}
