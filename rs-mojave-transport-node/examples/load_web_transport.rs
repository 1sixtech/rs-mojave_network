use anyhow::Context;
use clap::Parser;
use futures::StreamExt;
use libp2p_identity::Keypair;
use moq_native::server;
use rs_mojave_network_core::{muxing::StreamMuxerBox, transport::Transport};
use rs_mojave_transport_node::Builder;

#[derive(Parser, Clone)]
pub struct Config {
	/// Listen on this address, both TCP and UDP.
	#[arg(long, short = 'b', default_value = "[::]:443")]
	pub bind: String,

	/// The TLS configuration.
	#[command(flatten)]
	pub tls: server::ServerTlsConfig,
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
		.with_env_filter(filter)
		.with_file(true)
		.with_line_number(true)
		.with_target(true)
		.with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
		.init();

	tracing::info!("Creating a node with WebTransport...");

	let keypair = Keypair::generate_ed25519();
	let config = Config::parse();

	let bind = tokio::net::lookup_host(config.bind)
		.await
		.context("invalid bind address")?
		.next()
		.context("invalid bind address")?;

	let config = server::ServerConfig {
		listen: Some(bind),
		tls: config.tls,
	};

	let mut node = Builder::new(keypair)
		.with_transport(|keypair| {
			rs_mojave_transport_webtransport::WebTransport::new(config, true, keypair.clone())
				.map(|(peer_id, connection)| (peer_id, StreamMuxerBox::new(connection)))
				.boxed()
		})?
		.with_protocol(|_| rs_mojave_protocol_ping::Protocol::default())?
		.build();

	tracing::info!("Node created successfully with Peer ID: {}", node.peer_id);

	let address = "/ip4/0.0.0.0/udp/443/quic-v1/webtransport"
		.parse()
		.context("failed to parse the WebTransport address")?;

	node.listen(address).await?;

	loop {
		tokio::select! {
			event = node.next() => {
				tracing::trace!(?event)
			},
			_ = tokio::signal::ctrl_c() => {
				// TODO: Handle shutdown gracefully.
				tracing::info!("Ctrl+C received, shutting down");
				break;
			}
		}
	}

	Ok(())
}
