use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
	fmt::{Debug, Display},
	str::FromStr,
};

#[derive(Debug, thiserror::Error)]
pub enum ParseStreamProtocolError {
	#[error("Invalid protocol format: {0}")]
	MissingAt(String),
	#[error("Invalid protocol format: {0}")]
	MissingSlash(String),
	#[error("Invalid protocol version: {0}")]
	InvalidVersion(semver::Error),
}

/// Represents a protocol that can be used for streaming data over a network.
///
/// This struct encapsulates the protocol's name and version, allowing for
/// versioned protocol negotiation and identification.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct StreamProtocol {
	pub full: String,
	pub namespace: String,
	pub name: String,
	pub version: Version,
}

impl StreamProtocol {
	/// Creates a new `StreamProtocol` with the given name and version.
	///
	/// # Arguments
	///
	/// * `namespace` - The namespace of the protocol.
	/// * `name` - The name of the protocol.
	/// * `version` - The version of the protocol.
	pub fn new(namespace: &str, name: &str, version: Version) -> Self {
		let full = format!("{namespace}/{name}@{version}");
		Self {
			namespace: namespace.to_owned(),
			name: name.to_owned(),
			version,
			full,
		}
	}
}

impl FromStr for StreamProtocol {
	type Err = ParseStreamProtocolError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let (path, version_str) = s
			.rsplit_once('@')
			.ok_or(ParseStreamProtocolError::MissingAt(s.to_owned()))?;
		let (namespace, name) = path
			.split_once('/')
			.ok_or(ParseStreamProtocolError::MissingSlash(s.to_owned()))?;
		let version = Version::parse(version_str).map_err(ParseStreamProtocolError::InvalidVersion)?;

		Ok(Self {
			namespace: namespace.to_owned(),
			name: name.to_owned(),
			version,
			full: s.to_owned(),
		})
	}
}

impl Debug for StreamProtocol {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}/{}@{}", self.namespace, self.name, self.version)
	}
}

impl Display for StreamProtocol {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}/{}@{}", self.namespace, self.name, self.version)
	}
}

impl AsRef<str> for StreamProtocol {
	fn as_ref(&self) -> &str {
		&self.full
	}
}

impl Serialize for StreamProtocol {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.collect_str(self)
	}
}

impl<'de> Deserialize<'de> for StreamProtocol {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		struct StreamProtocolVisitor;

		impl<'de> serde::de::Visitor<'de> for StreamProtocolVisitor {
			type Value = StreamProtocol;

			fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
				formatter.write_str("a string in the format 'namespace/name@version'")
			}

			fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
			where
				E: serde::de::Error,
			{
				StreamProtocol::from_str(v).map_err(serde::de::Error::custom)
			}
		}

		deserializer.deserialize_str(StreamProtocolVisitor)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_new_stream_protocol() {
		let version = Version::parse("1.2.3").unwrap();
		let protocol = StreamProtocol::new("test", "protocol", version.clone());

		assert_eq!(protocol.namespace, "test");
		assert_eq!(protocol.name, "protocol");
		assert_eq!(protocol.version, version);
	}

	#[test]
	fn test_parse_valid_protocol_string() {
		let protocol_str = "test/protocol@1.2.3";
		let protocol = StreamProtocol::from_str(protocol_str).unwrap();

		assert_eq!(protocol.namespace, "test");
		assert_eq!(protocol.name, "protocol");
		assert_eq!(protocol.version, Version::parse("1.2.3").unwrap());
	}

	#[test]
	fn test_parse_missing_at_symbol() {
		let protocol_str = "test/protocol1.2.3";
		let error = StreamProtocol::from_str(protocol_str).unwrap_err();

		match error {
			ParseStreamProtocolError::MissingAt(s) => assert_eq!(s, protocol_str),
			_ => panic!("Expected MissingAt error"),
		}
	}

	#[test]
	fn test_parse_missing_slash() {
		let protocol_str = "testprotocol@1.2.3";
		let error = StreamProtocol::from_str(protocol_str).unwrap_err();

		match error {
			ParseStreamProtocolError::MissingSlash(s) => assert_eq!(s, protocol_str),
			_ => panic!("Expected MissingSlash error"),
		}
	}

	#[test]
	fn test_parse_invalid_version() {
		let protocol_str = "test/protocol@invalid";
		let error = StreamProtocol::from_str(protocol_str).unwrap_err();

		match error {
			ParseStreamProtocolError::InvalidVersion(_) => {}
			_ => panic!("Expected InvalidVersion error"),
		}
	}

	#[test]
	fn test_debug_format() {
		let version = Version::parse("1.2.3").unwrap();
		let protocol = StreamProtocol::new("test", "protocol", version);

		assert_eq!(format!("{protocol}"), "test/protocol@1.2.3");
	}

	#[test]
	fn test_display_format() {
		let version = Version::parse("1.2.3").unwrap();
		let protocol = StreamProtocol::new("test", "protocol", version);

		assert_eq!(format!("{protocol}"), "test/protocol@1.2.3");
	}

	#[test]
	fn test_protocol_equality() {
		let version1 = Version::parse("1.2.3").unwrap();
		let version2 = Version::parse("1.2.3").unwrap();
		let protocol1 = StreamProtocol::new("test", "protocol", version1);
		let protocol2 = StreamProtocol::new("test", "protocol", version2);

		assert_eq!(protocol1, protocol2);
	}

	#[test]
	fn test_protocol_inequality() {
		let version1 = Version::parse("1.2.3").unwrap();
		let version2 = Version::parse("1.2.4").unwrap();
		let protocol1 = StreamProtocol::new("test", "protocol", version1.clone());
		let protocol2 = StreamProtocol::new("test", "protocol", version2);

		assert_ne!(protocol1, protocol2);

		let protocol3 = StreamProtocol::new("different", "protocol", version1.clone());
		assert_ne!(protocol1, protocol3);

		let protocol4 = StreamProtocol::new("test", "different", version1);
		assert_ne!(protocol1, protocol4);
	}

	#[test]
	fn test_complex_protocol_parsing() {
		let protocol_str = "org.example/my-protocol@2.0.0-alpha.1+build.2023";
		let protocol = StreamProtocol::from_str(protocol_str).unwrap();

		assert_eq!(protocol.namespace, "org.example");
		assert_eq!(protocol.name, "my-protocol");
		assert_eq!(protocol.version, Version::parse("2.0.0-alpha.1+build.2023").unwrap());
	}

	#[test]
	fn test_clone() {
		let version = Version::parse("1.2.3").unwrap();
		let protocol = StreamProtocol::new("test", "protocol", version);
		let cloned = protocol.clone();

		assert_eq!(protocol, cloned);
	}

	#[test]
	fn test_serialization() {
		let version = Version::parse("1.2.3").unwrap();
		let protocol = StreamProtocol::new("test", "protocol", version);

		// Test JSON serialization
		let serialized = serde_json::to_string(&protocol).unwrap();
		assert_eq!(serialized, "\"test/protocol@1.2.3\"");

		// Test with complex protocol
		let complex_version = Version::parse("2.0.0-alpha.1+build.2023").unwrap();
		let complex_protocol = StreamProtocol::new("org.example", "my-protocol", complex_version);
		let serialized = serde_json::to_string(&complex_protocol).unwrap();
		assert_eq!(serialized, "\"org.example/my-protocol@2.0.0-alpha.1+build.2023\"");
	}

	#[test]
	fn test_deserialization() {
		// Test basic deserialization
		let json = "\"test/protocol@1.2.3\"";
		let protocol: StreamProtocol = serde_json::from_str(json).unwrap();

		assert_eq!(protocol.namespace, "test");
		assert_eq!(protocol.name, "protocol");
		assert_eq!(protocol.version, Version::parse("1.2.3").unwrap());

		// Test with complex protocol
		let complex_json = "\"org.example/my-protocol@2.0.0-alpha.1+build.2023\"";
		let protocol: StreamProtocol = serde_json::from_str(complex_json).unwrap();

		assert_eq!(protocol.namespace, "org.example");
		assert_eq!(protocol.name, "my-protocol");
		assert_eq!(protocol.version, Version::parse("2.0.0-alpha.1+build.2023").unwrap());
	}

	#[test]
	fn test_invalid_deserialization() {
		// Test missing @ symbol
		let invalid_json = "\"test/protocol1.2.3\"";
		let result: Result<StreamProtocol, _> = serde_json::from_str(invalid_json);
		assert!(result.is_err());

		// Test missing slash
		let invalid_json = "\"testprotocol@1.2.3\"";
		let result: Result<StreamProtocol, _> = serde_json::from_str(invalid_json);
		assert!(result.is_err());

		// Test invalid version
		let invalid_json = "\"test/protocol@invalid\"";
		let result: Result<StreamProtocol, _> = serde_json::from_str(invalid_json);
		assert!(result.is_err());
	}

	#[test]
	fn test_round_trip_serialization() {
		// Test that a protocol can be serialized and then deserialized back to the original
		let version = Version::parse("1.2.3").unwrap();
		let original = StreamProtocol::new("test", "protocol", version);

		let serialized = serde_json::to_string(&original).unwrap();
		let deserialized: StreamProtocol = serde_json::from_str(&serialized).unwrap();

		assert_eq!(original, deserialized);

		// Test with complex protocol
		let complex_version = Version::parse("2.0.0-alpha.1+build.2023").unwrap();
		let original = StreamProtocol::new("org.example", "my-protocol", complex_version);

		let serialized = serde_json::to_string(&original).unwrap();
		let deserialized: StreamProtocol = serde_json::from_str(&serialized).unwrap();

		assert_eq!(original, deserialized);
	}
}
