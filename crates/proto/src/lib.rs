pub mod jsonrpc;
pub mod marketplace;
pub mod methods;
pub mod path_component;
pub mod types;

pub use jsonrpc::{ErrorCode, ErrorObject, Notification, Request, Response, RpcEnvelope};
pub use types::*;

pub const PROTOCOL_VERSION: &str = "0.1";
pub const SERVER_NAME: &str = "joi-server";
pub const SERVER_TITLE: &str = "Joi Apps Server";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
