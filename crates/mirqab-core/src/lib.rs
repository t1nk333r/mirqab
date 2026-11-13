//! The daemon's library: root, exec, act, server, samplers and history
//! (architecture §5, §6).

pub use mirqab_proto as proto;

pub mod act;
pub mod config;
pub mod exec;
pub mod history;
pub mod root;
pub mod sampler;
pub mod samplers;
pub mod selfcost;
pub mod server;
pub mod sys;

pub use root::Root;
