// Streams module - some types for future use

pub mod client;
pub mod config;

pub use client::{StreamClient, StreamData, StreamEvent, StreamManager};
pub use config::{ConnectionState, StreamsConfig};
