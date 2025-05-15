/// Recording metrics using VictoriaMetrics.
///
/// This module provides types for sending metrics to a VictoriaMetrics/InfluxDB
/// database, using the InfluxDB line protocol over UDP.
use std::fmt;
use std::io::Error;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::Mutex;

/// A metric to store in the metrics database.
pub(crate) struct Metric<'a> {
    name: &'a str,
    tags: Vec<(&'a str, String)>,
    fields: Vec<(&'a str, String)>,
}

impl<'a> Metric<'a> {
    pub(crate) fn with(name: &'a str, func: impl FnOnce(&mut Self)) -> Self {
        let mut metric = Self::new(name);

        func(&mut metric);
        metric
    }

    pub(crate) fn new(name: &'a str) -> Self {
        Metric { name, tags: Vec::new(), fields: Vec::new() }
    }

    pub(crate) fn field(&mut self, name: &'a str, value: u64) {
        self.fields.push((name, value.to_string()));
    }

    pub(crate) fn tag(&mut self, name: &'a str, value: &str) {
        self.tags.push((name, value.to_string()));
    }
}

impl<'a> fmt::Display for Metric<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.name)?;

        if !self.tags.is_empty() {
            f.write_str(",")?;

            for (index, (name, val)) in self.tags.iter().enumerate() {
                if index > 0 {
                    f.write_str(",")?;
                }

                write!(f, "{}={}", name, val)?;
            }
        }

        if !self.fields.is_empty() {
            f.write_str(",")?;

            for (index, (name, val)) in self.fields.iter().enumerate() {
                if index > 0 {
                    f.write_str(",")?;
                }

                write!(f, "{}={}", name, val)?;
            }
        }

        Ok(())
    }
}

/// A type for sending metrics to a server.
pub(crate) struct Metrics {
    /// The address to send the metrics to.
    ///
    /// Since we're using an UDP socket we have to keep this address around such
    /// that we can pass it to `UdpSocket::send_to()`.
    addr: SocketAddr,

    /// The socket to use for sending metrics.
    ///
    /// We use interior mutability so the use of a socket can be kept private to
    /// this type, making it easier to use by the various threads.
    socket: Mutex<UdpSocket>,
}

impl Metrics {
    /// Connects to the metrics database at the given address.
    pub(crate) fn new(ip: IpAddr, port: u16) -> Result<Arc<Self>, Error> {
        let addr = SocketAddr::new(ip, port);
        let sock = UdpSocket::bind("0.0.0.0:0")?;

        Ok(Arc::new(Self { addr: addr, socket: Mutex::new(sock) }))
    }

    pub(crate) fn add(&self, name: &str, func: impl FnOnce(&mut Metric)) {
        self.send(Metric::with(name, func));
    }

    fn send(&self, metric: Metric) {
        // If we encounter a poisoned mutex there's nothing we can (nor should)
        // do but abort.
        let socket = self.socket.lock().unwrap();

        // If we fail to send the metrics that's OK, as it doesn't affect the
        // rest of the system.
        let _ = socket.send_to(metric.to_string().as_bytes(), self.addr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_field() {
        let mut metric = Metric::new("example");

        metric.field("key", 10);
        assert_eq!(metric.fields, vec![("key", 10.to_string())]);
    }

    #[test]
    fn test_metric_tag() {
        let mut metric = Metric::new("example");

        metric.tag("key", "value");
        assert_eq!(metric.tags, vec![("key", "value".to_string())]);
    }

    #[test]
    fn test_metric_to_string() {
        let mut metric = Metric::new("example");

        metric.field("field1", 10);
        metric.field("field2", 20);
        metric.tag("tag1", "value1");
        metric.tag("tag2", "value2");

        assert_eq!(
            metric.to_string(),
            "example,tag1=value1,tag2=value2,field1=10,field2=20".to_string()
        );
    }
}
