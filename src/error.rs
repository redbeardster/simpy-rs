//! Типы ошибок для симуляции

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SimError {
    #[error("Lua error: {0}")]
    LuaError(#[from] mlua::Error),

    #[error("Simulation error: {0}")]
    SimulationError(String),

    #[error("Resource error: {0}")]
    ResourceError(String),

    #[error("Process error: {0}")]
    ProcessError(String),
}

impl From<String> for SimError {
    fn from(s: String) -> Self {
        SimError::SimulationError(s)
    }
}

impl From<&str> for SimError {
    fn from(s: &str) -> Self {
        SimError::SimulationError(s.to_string())
    }
}
