//! Управление ресурсами симуляции

use std::collections::{HashMap, VecDeque};
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    name: String,
    capacity: usize,
    available: usize,
    queue_length: usize,
    total_requests: u64,
    total_wait_time: f64, // суммарное время ожидания
}

impl Resource {
    fn new(name: &str, capacity: usize) -> Self {
        Self {
            name: name.to_string(),
            capacity,
            available: capacity,
            queue_length: 0,
            total_requests: 0,
            total_wait_time: 0.0,
        }
    }
}

pub struct ResourceManager {
    resources: HashMap<String, Resource>,
    request_queues: HashMap<String, VecDeque<String>>, // resource -> очередь процессов
}

impl ResourceManager {
    pub fn new() -> Self {
        Self {
            resources: HashMap::new(),
            request_queues: HashMap::new(),
        }
    }

    pub fn create(&mut self, name: &str, capacity: usize) {
        self.resources.insert(name.to_string(), Resource::new(name, capacity));
        self.request_queues.insert(name.to_string(), VecDeque::new());
    }

    /// Попытка получить ресурс. Возвращает true, если ресурс получен немедленно
    pub fn request(&mut self, resource_name: &str) -> bool {
        if let Some(resource) = self.resources.get_mut(resource_name) {
            if resource.available > 0 {
                resource.available -= 1;
                resource.total_requests += 1;
                true
            } else {
                // Встаем в очередь
                if let Some(queue) = self.request_queues.get_mut(resource_name) {
                    resource.queue_length = queue.len() + 1;
                }
                false
            }
        } else {
            false
        }
    }

    /// Освободить ресурс
    pub fn release(&mut self, resource_name: &str) {
        if let Some(resource) = self.resources.get_mut(resource_name) {
            if resource.available < resource.capacity {
                resource.available += 1;

                // Проверяем очередь
                if let Some(queue) = self.request_queues.get_mut(resource_name) {
                    resource.queue_length = queue.len();
                }
            }
        }
    }

    /// Добавить процесс в очередь ожидания
    pub fn queue_request(&mut self, resource_name: &str, process_name: &str) {
        if let Some(queue) = self.request_queues.get_mut(resource_name) {
            queue.push_back(process_name.to_string());

            if let Some(resource) = self.resources.get_mut(resource_name) {
                resource.queue_length = queue.len();
            }
        }
    }

    /// Получить статистику по ресурсам
    pub fn get_stats(&self) -> Vec<serde_json::Value> {
        self.resources
            .values()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "capacity": r.capacity,
                    "available": r.available,
                    "utilization": (r.capacity - r.available) as f64 / r.capacity as f64,
                    "queue_length": r.queue_length,
                    "total_requests": r.total_requests,
                })
            })
            .collect()
    }
}
