use multiaddr::Multiaddr;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConnectionOrigin {
	Dialer {
		remote_addr: Multiaddr,
	},
	Listener {
		local_addr: Multiaddr,
		remote_addr: Multiaddr,
	},
}
