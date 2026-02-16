//! Основное ядро симуляции

use std::collections::BinaryHeap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, debug};

use super::event::{Event, Priority};
use super::time::{SimTime, Duration};
use crate::SimError;

/// Основной симулятор
pub struct Simulation {
    current_time: Arc<Mutex<SimTime>>,
    event_queue: Arc<Mutex<BinaryHeap<Event>>>,
    event_counter: Arc<Mutex<u64>>,
}

impl Simulation {
    pub fn new() -> Self {
        Self {
            current_time: Arc::new(Mutex::new(SimTime::ZERO)),
            event_queue: Arc::new(Mutex::new(BinaryHeap::new())),
            event_counter: Arc::new(Mutex::new(0)),
        }
    }

    pub async fn now(&self) -> SimTime {
        *self.current_time.lock().await
    }

    pub async fn set_time(&self, time: SimTime) {
        let mut current = self.current_time.lock().await;
        *current = time;
    }


    pub async fn schedule_after<F>(
        &self,
        delay: Duration,
        priority: Priority,
        callback: F,
    ) -> Result<(), SimError>
    where
        F: FnOnce() + Send + 'static,
    {
        let current = self.now().await;
        let event_time = current + SimTime::new(delay.as_seconds());
        self.schedule_at(event_time, priority, callback).await
    }

    pub async fn schedule_at<F>(
        &self,
        time: SimTime,
        priority: Priority,
        callback: F,
    ) -> Result<(), SimError>
    where
        F: FnOnce() + Send + 'static,
    {
        let mut counter = self.event_counter.lock().await;
        let id = *counter;
        *counter += 1;

        let event = Event::new(time, priority, id, callback);

        let mut queue = self.event_queue.lock().await;
        queue.push(event);

        debug!("Событие запланировано на время {}", time);
        Ok(())
    }

    pub async fn run_until(&mut self, end_time: SimTime) -> Result<(), SimError> {
        info!("Запуск симуляции до времени {}", end_time);

        while self.now().await < end_time && self.has_events().await {
            self.process_next_event().await?;
        }

        info!("Симуляция завершена. Финальное время: {}", self.now().await);
        Ok(())
    }

    pub async fn run_for(&mut self, duration: Duration) -> Result<(), SimError> {
        let start = self.now().await;
        let end = start + SimTime::new(duration.as_seconds());
        self.run_until(end).await
    }

    pub async fn process_next_event(&self) -> Result<(), SimError> {
        let next_event = {
            let mut queue = self.event_queue.lock().await;
            queue.pop()
        };

        if let Some(event) = next_event {
            {
                let mut current = self.current_time.lock().await;
                *current = event.time;
            }

            debug!("Обработка события в {}", event.time);
            (event.callback)();

            Ok(())
        } else {
            Err(SimError::SimulationError("Нет событий в очереди".to_string()))
        }
    }

    pub async fn has_events(&self) -> bool {
        !self.event_queue.lock().await.is_empty()
    }

    pub async fn clear_events(&self) {
        let mut queue = self.event_queue.lock().await;
        queue.clear();
        debug!("Очередь событий очищена");
    }
}

impl Default for Simulation {
    fn default() -> Self {
        Self::new()
    }
}
