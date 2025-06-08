use std::{io, sync::LazyLock, time::Duration};

use futures::prelude::*;
use rand::{distr, prelude::*, rng};
use rs_mojave_transport_node::StreamProtocol;
use semver::Version;
use web_time::Instant;

pub const PROTOCOL_VERSION: Version = Version::new(0, 0, 1);
pub static PROTOCOL_NAME: LazyLock<StreamProtocol> =
	LazyLock::new(|| StreamProtocol::new("rs-mojave", "ping", PROTOCOL_VERSION));

const PING_SIZE: usize = 32;

pub(crate) async fn send_ping<S>(mut stream: S) -> io::Result<(S, Duration)>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	let payload: [u8; PING_SIZE] = rng().sample(distr::StandardUniform);
	stream.write_all(&payload).await?;
	stream.flush().await?;
	let started = Instant::now();
	let mut recv_payload = [0u8; PING_SIZE];
	stream.read_exact(&mut recv_payload).await?;
	if recv_payload == payload {
		Ok((stream, started.elapsed()))
	} else {
		Err(io::Error::new(io::ErrorKind::InvalidData, "Ping payload mismatch"))
	}
}

/// Waits for a ping and sends a pong.
pub(crate) async fn recv_ping<S>(mut stream: S) -> io::Result<S>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	let mut payload = [0u8; PING_SIZE];
	stream.read_exact(&mut payload).await?;
	stream.write_all(&payload).await?;
	stream.flush().await?;
	Ok(stream)
}
