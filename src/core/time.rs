//! Управление временем симуляции

use std::fmt;
use std::ops::{Add, Sub};
use serde::{Serialize, Deserialize};

/// Тип для представления времени в симуляции
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SimTime(f64);

impl SimTime {
    pub const ZERO: SimTime = SimTime(0.0);

    pub fn new(seconds: f64) -> Self {
        SimTime(seconds.max(0.0))
    }

    pub fn as_seconds(&self) -> f64 {
        self.0
    }

    pub fn from_seconds(seconds: f64) -> Self {
        SimTime(seconds)
    }
}

impl Add for SimTime {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        SimTime(self.0 + other.0)
    }
}

impl Sub for SimTime {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        SimTime((self.0 - other.0).max(0.0))
    }
}

impl fmt::Display for SimTime {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:.3}s", self.0)
    }
}

/// Длительность для asynchronix
#[derive(Debug, Clone, Copy)]
pub struct Duration(f64);

impl Duration {
    pub fn from_seconds(secs: f64) -> Self {
        Duration(secs)
    }

    pub fn as_seconds(&self) -> f64 {
        self.0
    }
}
