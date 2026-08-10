//! Typed, bounded grammar for the native host-neutral child protocol.
//!
//! A session is created only after verification, authentication, and grants.
//! Parsing never accepts SQL, paths, database bytes, or generic host methods.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;

pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_REQUESTS_PER_SESSION: u64 = 4096;
const SESSION_TOKEN_CHARS: usize = 43;
const MAX_ID_CHARS: usize = 64;
const MAX_ENDPOINT_CHARS: usize = 128;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolMethod {
    Manifest,
    Permissions,
    Read,
    Write,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProtocolParams {
    Manifest,
    Permissions,
    Read(NamedEndpointParams),
    Write(NamedEndpointParams),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NamedEndpointParams {
    pub endpoint: String,
    pub arguments: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolRequest {
    pub sequence: u64,
    pub id: String,
    pub method: ProtocolMethod,
    pub params: ProtocolParams,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestWire {
    version: u8,
    session: String,
    sequence: u64,
    id: String,
    method: ProtocolMethod,
    params: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyParams {}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("request exceeds the {MAX_REQUEST_BYTES} byte limit")]
    TooLarge,
    #[error("request is not valid protocol JSON")]
    Malformed,
    #[error("unsupported protocol version")]
    Version,
    #[error("session token is invalid or stale")]
    Session,
    #[error("sequence is invalid, stale, or out of order")]
    Sequence,
    #[error("request id is invalid or duplicated")]
    RequestId,
    #[error("named endpoint is invalid")]
    Endpoint,
    #[error("session request limit reached")]
    SessionExhausted,
}

#[derive(Debug)]
pub struct ProtocolSession {
    token: String,
    last_sequence: u64,
    request_ids: HashSet<String>,
}

impl ProtocolSession {
    pub fn new(token: String) -> Result<Self, ProtocolError> {
        if !valid_session_token(&token) {
            return Err(ProtocolError::Session);
        }
        Ok(Self {
            token,
            last_sequence: 0,
            request_ids: HashSet::new(),
        })
    }

    pub fn accept(&mut self, bytes: &[u8]) -> Result<ProtocolRequest, ProtocolError> {
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(ProtocolError::TooLarge);
        }
        if self.last_sequence >= MAX_REQUESTS_PER_SESSION {
            return Err(ProtocolError::SessionExhausted);
        }

        let wire: RequestWire =
            serde_json::from_slice(bytes).map_err(|_| ProtocolError::Malformed)?;
        if wire.version != PROTOCOL_VERSION {
            return Err(ProtocolError::Version);
        }
        if !valid_session_token(&wire.session) || wire.session != self.token {
            return Err(ProtocolError::Session);
        }
        if wire.sequence != self.last_sequence + 1 {
            return Err(ProtocolError::Sequence);
        }
        if !valid_name(&wire.id, MAX_ID_CHARS) || self.request_ids.contains(&wire.id) {
            return Err(ProtocolError::RequestId);
        }

        let params = match wire.method {
            ProtocolMethod::Manifest | ProtocolMethod::Permissions => {
                serde_json::from_value::<EmptyParams>(wire.params)
                    .map_err(|_| ProtocolError::Malformed)?;
                if wire.method == ProtocolMethod::Manifest {
                    ProtocolParams::Manifest
                } else {
                    ProtocolParams::Permissions
                }
            }
            ProtocolMethod::Read | ProtocolMethod::Write => {
                let params = serde_json::from_value::<NamedEndpointParams>(wire.params)
                    .map_err(|_| ProtocolError::Malformed)?;
                if !valid_name(&params.endpoint, MAX_ENDPOINT_CHARS) {
                    return Err(ProtocolError::Endpoint);
                }
                if wire.method == ProtocolMethod::Read {
                    ProtocolParams::Read(params)
                } else {
                    ProtocolParams::Write(params)
                }
            }
        };

        self.last_sequence = wire.sequence;
        self.request_ids.insert(wire.id.clone());
        Ok(ProtocolRequest {
            sequence: wire.sequence,
            id: wire.id,
            method: wire.method,
            params,
        })
    }
}

fn valid_session_token(value: &str) -> bool {
    value.len() == SESSION_TOKEN_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_name(value: &str, max_chars: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_chars
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGH012345678";

    fn session() -> ProtocolSession {
        ProtocolSession::new(TOKEN.to_owned()).expect("valid fixture token")
    }

    fn request(sequence: u64, id: &str, method: &str, params: &str) -> Vec<u8> {
        format!(
            r#"{{"version":1,"session":"{TOKEN}","sequence":{sequence},"id":"{id}","method":"{method}","params":{params}}}"#
        )
        .into_bytes()
    }

    #[test]
    fn accepts_exact_manifest_and_named_endpoint_requests() {
        let mut session = session();
        let manifest = session
            .accept(&request(1, "manifest-1", "manifest", "{}"))
            .expect("manifest");
        assert_eq!(manifest.params, ProtocolParams::Manifest);

        let permissions = session
            .accept(&request(2, "permissions-1", "permissions", "{}"))
            .expect("permissions");
        assert_eq!(permissions.params, ProtocolParams::Permissions);

        let read = session
            .accept(&request(
                3,
                "read-1",
                "read",
                r#"{"endpoint":"diagram.snapshot","arguments":{"id":"main"}}"#,
            ))
            .expect("read");
        assert!(matches!(read.params, ProtocolParams::Read(_)));

        let write = session
            .accept(&request(
                4,
                "write-1",
                "write",
                r#"{"endpoint":"node.rename","arguments":{"id":"n1","label":"A"}}"#,
            ))
            .expect("write");
        assert!(matches!(write.params, ProtocolParams::Write(_)));
    }

    #[test]
    fn rejects_unknown_and_duplicate_fields() {
        let mut session = session();
        let unknown = format!(
            r#"{{"version":1,"session":"{TOKEN}","sequence":1,"id":"x","method":"manifest","params":{{}},"extra":true}}"#
        );
        assert_eq!(
            session.accept(unknown.as_bytes()),
            Err(ProtocolError::Malformed)
        );

        let duplicate = format!(
            r#"{{"version":1,"version":1,"session":"{TOKEN}","sequence":1,"id":"x","method":"manifest","params":{{}}}}"#
        );
        assert_eq!(
            session.accept(duplicate.as_bytes()),
            Err(ProtocolError::Malformed)
        );
    }

    #[test]
    fn rejects_stale_sessions_sequences_and_ids() {
        let mut session = session();
        let wrong_session = br#"{"version":1,"session":"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefgh012345678","sequence":1,"id":"x","method":"manifest","params":{}}"#;
        assert_eq!(session.accept(wrong_session), Err(ProtocolError::Session));

        session
            .accept(&request(1, "x", "manifest", "{}"))
            .expect("first request");
        assert_eq!(
            session.accept(&request(1, "y", "manifest", "{}")),
            Err(ProtocolError::Sequence)
        );
        assert_eq!(
            session.accept(&request(2, "x", "manifest", "{}")),
            Err(ProtocolError::RequestId)
        );
    }

    #[test]
    fn rejects_unknown_params_and_oversized_bodies() {
        let mut session = session();
        assert_eq!(
            session.accept(&request(1, "x", "manifest", r#"{"extra":true}"#)),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            session.accept(&vec![b'x'; MAX_REQUEST_BYTES + 1]),
            Err(ProtocolError::TooLarge)
        );
    }
}
