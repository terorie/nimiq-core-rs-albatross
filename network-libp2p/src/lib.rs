#![feature(ip)] // For IpAddr::is_global

#[macro_use]
extern crate beserial_derive;
#[macro_use]
extern crate log;

mod behaviour;
pub mod discovery;
mod limit;
pub mod message;
pub mod message_codec;
mod network;
pub mod task;

pub const MESSAGE_PROTOCOL: &[u8] = b"/nimiq/message/0.0.1";
pub const DISCOVERY_PROTOCOL: &[u8] = b"/nimiq/discovery/0.0.1";
pub const LIMIT_PROTOCOL: &[u8] = b"/nimiq/limit/0.0.1";

pub use libp2p::{self, core::network::NetworkInfo, identity::Keypair, Multiaddr};

pub use network::{Config, Network, NetworkError};
