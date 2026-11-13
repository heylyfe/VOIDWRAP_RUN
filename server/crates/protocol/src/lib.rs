//! VOIDWRAP RUN wire protocol.
//!
//! All message types are generated from `protocol/schema.json` (see `generated.rs`).
//! The wire format is bincode 1.x with fixint little-endian encoding; the TypeScript
//! codec in `web/src/protocol` implements the same layout, and golden fixtures in
//! `protocol/fixtures` are checked from both languages.

#[rustfmt::skip]
mod generated;

use bincode::Options;
pub use generated::*;

/// Encoding/decoding failure.
pub type CodecError = bincode::Error;

fn options() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
}

/// Encode any protocol value.
pub fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    options().serialize(value)
}

/// Decode a value from a trusted source (data this program produced). Use
/// [`decode_client_msg`] for anything that came off the network.
pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    options().deserialize(bytes)
}

/// Decode an untrusted client message. Rejects oversized frames up front and caps
/// every allocation the decoder may perform at [`MAX_CLIENT_MSG_BYTES`].
pub fn decode_client_msg(bytes: &[u8]) -> Result<ClientMsg, CodecError> {
    let limit = u64::from(MAX_CLIENT_MSG_BYTES);
    if bytes.len() as u64 > limit {
        return Err(Box::new(bincode::ErrorKind::SizeLimit));
    }
    options().with_limit(limit).deserialize(bytes)
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }
}

impl Default for Tuning {
    /// Neutral tuning: the base movement with no upgrades or curses.
    fn default() -> Self {
        Tuning {
            run_speed_mul: 1.0,
            jump_mul: 1.0,
            gravity_mul: 1.0,
            dash_speed_mul: 1.0,
            dash_cooldown_mul: 1.0,
            air_jumps: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_protocol_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../protocol")
    }

    fn to_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Golden fixtures: the JSON values in schema.json must encode to exactly the
    /// bytes in protocol/fixtures/*.hex. The TS test suite checks the same files.
    /// Set VW_UPDATE_FIXTURES=1 to (re)write them.
    #[test]
    fn golden_fixtures_match() {
        let dir = repo_protocol_dir();
        let schema: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("schema.json")).unwrap()).unwrap();
        let update = std::env::var_os("VW_UPDATE_FIXTURES").is_some();
        let fixtures = schema["fixtures"].as_array().unwrap();
        assert!(!fixtures.is_empty());
        for fx in fixtures {
            let name = fx["name"].as_str().unwrap();
            let ty = fx["type"].as_str().unwrap();
            let bytes =
                generated::fixture_encode(ty, fx["value"].clone()).unwrap_or_else(|e| panic!("fixture {name}: {e}"));
            let path = dir.join("fixtures").join(format!("{name}.hex"));
            if update {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, to_hex(&bytes) + "\n").unwrap();
            } else {
                let want =
                    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing {path:?}; run `just fixtures`"));
                assert_eq!(to_hex(&bytes), want.trim(), "fixture {name} drifted");
            }
        }
    }

    #[test]
    fn client_msg_roundtrip() {
        let msg = ClientMsg::Inputs {
            frames: vec![InputFrame {
                seq: 1,
                move_x: -1,
                move_z: 127,
                buttons: BTN_JUMP,
            }],
        };
        let bytes = encode(&msg).unwrap();
        assert_eq!(decode_client_msg(&bytes).unwrap(), msg);
    }

    #[test]
    fn hostile_length_prefix_is_rejected_without_allocating() {
        // Inputs variant (tag 1) claiming u64::MAX frames.
        let mut bytes = 1u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&u64::MAX.to_le_bytes());
        assert!(decode_client_msg(&bytes).is_err());
    }

    #[test]
    fn oversized_and_trailing_input_rejected() {
        let big = vec![0u8; MAX_CLIENT_MSG_BYTES as usize + 1];
        assert!(decode_client_msg(&big).is_err());
        let mut ok = encode(&ClientMsg::Restart).unwrap();
        ok.push(0);
        assert!(decode_client_msg(&ok).is_err());
    }

    #[test]
    fn unknown_variant_rejected() {
        assert!(decode_client_msg(&99u32.to_le_bytes()).is_err());
    }
}
