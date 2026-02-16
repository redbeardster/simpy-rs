//! SimPy-rs - Дискретно-событийное моделирование на Rust с Lua скриптингом

pub mod core;
pub mod lua;
pub mod resources;
pub mod error;

mod simulator;
pub use simulator::Simulator;
pub use error::SimError;

pub mod prelude {
    pub use crate::core::SimTime;
    pub use crate::Simulator;
    pub use crate::SimError;
}
