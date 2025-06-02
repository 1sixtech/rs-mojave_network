use anyhow::Context;
use clap::Parser;
use futures::StreamExt;
use libp2p_identity::Keypair;
use moq_native::quic;
use rs_mojave_network_core::{muxing::StreamMuxerBox, transport::Transport};
use rs_mojave_transport_node::Builder;
use tracing::info;

#[derive(Parser, Clone)]
pub struct Config {
	/// Listen on this address, both TCP and UDP.
	#[arg(long, short = 'b', default_value = "[::]:443")]
	pub bind: String,

	/// The TLS configuration.
	#[command(flatten)]
	pub tls: moq_native::tls::Args,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	let filter = tracing_subscriber::EnvFilter::builder()
		.with_default_directive(tracing::Level::INFO.into())
		.from_env_lossy()
		.add_directive("quinn=info".parse().unwrap())
		.add_directive("h2=warn".parse().unwrap())
		.add_directive("web_transport_quinn=info".parse().unwrap())
		.add_directive("rustls=info".parse().unwrap())
		.add_directive("reqwest=info".parse().unwrap())
		.add_directive("hyper_util=info".parse().unwrap());

	tracing_subscriber::fmt()
		.with_file(true)
		.with_line_number(true)
		.with_target(true)
		.with_env_filter(filter)
		.with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
		.init();

	info!("Creating a node with WebTransport...");

	let keypair = Keypair::generate_ed25519();
	let config = Config::parse();

	let bind = tokio::net::lookup_host(config.bind)
		.await
		.context("invalid bind address")?
		.next()
		.context("invalid bind address")?;

	let tls = config.tls.load()?;

	if tls.server.is_none() {
		anyhow::bail!("missing TLS certificates");
	}

	let config = quic::Config { bind, tls };

	let mut node = Builder::new(keypair)
		.with_transport(|keypair| {
			rs_mojave_transport_webtransport::WebTransport::new(config, true, keypair.clone())
				.map(|(peer_id, connection)| (peer_id, StreamMuxerBox::new(connection)))
				.boxed()
		})?
		.build();

	tracing::info!("Node created successfully with Peer ID: {}", node.peer_id);

	let address =
		"/ip4/127.0.0.1/udp/443/quic-v1/webtransport/p2p/12D3KooWQWBgSAg1Z4kjoonCwSmCwmtbP4ZQFAYyna6oYQPLhc8i"
			.parse()?;

	let peer_id = "12D3KooWQWBgSAg1Z4kjoonCwSmCwmtbP4ZQFAYyna6oYQPLhc8i".parse()?;

	node.dial(peer_id, address).await?;

	loop {
		tokio::select! {
			event = node.next() => {
				tracing::trace!(?event)
			},
			_ = tokio::signal::ctrl_c() => {
				// TODO: Handle shutdown gracefully.
				info!("Ctrl+C received, shutting down");
				break;
			}
		}
	}
	Ok(())
}
