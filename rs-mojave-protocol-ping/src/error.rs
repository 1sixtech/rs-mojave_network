#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("timeout {0}")]
	Timeout(u64),
	#[error("unsupported protocol")]
	UnsupportedProtocol,
	#[error("Other error: {0}")]
	Other(#[from] Box<dyn std::error::Error + Send + Sync + 'static>),
	#[error(transparent)]
	Io(#[from] std::io::Error),
}
