//! Система событий для симуляции

use std::cmp::Ordering;
use super::time::SimTime;

/// Приоритет события (меньше = важнее)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    High = 0,
    Normal = 1,
    Low = 2,
}

/// Событие в очереди симуляции
pub struct Event {
    pub time: SimTime,
    pub priority: Priority,
    pub id: u64,  // Для уникальности при сравнении
    pub callback: Box<dyn FnOnce() + Send>,
}

impl Event {
    pub fn new<F>(time: SimTime, priority: Priority, id: u64, callback: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self {
            time,
            priority,
            id,
            callback: Box::new(callback),
        }
    }
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time && self.priority == other.priority && self.id == other.id
    }
}

impl Eq for Event {}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Event {
    fn cmp(&self, other: &Self) -> Ordering {
        // Для BinaryHeap нам нужен обратный порядок (меньшее время = выше приоритет)
        match other.time.partial_cmp(&self.time) {
            Some(Ordering::Equal) => {
                match other.priority.cmp(&self.priority) {
                    Ordering::Equal => other.id.cmp(&self.id),
                    other => other,
                }
            }
            Some(ordering) => ordering,
            None => Ordering::Equal,
        }
    }
}
