//! Ядро симуляции

mod simulation;
mod event;
mod time;

pub use simulation::Simulation;
pub use event::Priority;  // Добавляем экспорт Priority
pub use time::{SimTime, Duration};
