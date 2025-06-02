#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionOrigin {
	Dialer,
	Listener,
}
