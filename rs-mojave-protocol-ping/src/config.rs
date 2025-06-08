use std::time::Duration;

/// The configuration for outbound pings.
#[derive(Debug, Clone)]
pub struct Config {
	/// The timeout of an outbound ping.
	timeout: Duration,
	/// The duration between outbound pings.
	interval: Duration,
}

impl Config {
	/// Creates a new [`Config`] with the following default settings:
	///
	///   * [`Config::with_interval`] 15s
	///   * [`Config::with_timeout`] 20s
	///
	/// These settings have the following effect:
	///
	///   * A ping is sent every 15 seconds on a healthy connection.
	///   * Every ping sent must yield a response within 20 seconds in order to be successful.
	pub fn new() -> Self {
		Self {
			timeout: Duration::from_secs(20),
			interval: Duration::from_secs(15),
		}
	}

	/// Sets the ping timeout.
	pub fn with_timeout(mut self, d: Duration) -> Self {
		self.timeout = d;
		self
	}

	/// Sets the ping interval.
	pub fn with_interval(mut self, d: Duration) -> Self {
		self.interval = d;
		self
	}
}

impl Default for Config {
	fn default() -> Self {
		Self::new()
	}
}
