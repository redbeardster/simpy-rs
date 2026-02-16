//! Полноценная симуляция с Lua скриптингом

use crate::core::{Simulation, SimTime};
use crate::lua::{LuaEngine, ProcessMessage, LuaCommand, LogLevel};
use crate::resources::ResourceManager;
use crate::SimError;

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, debug, warn, error};
use serde_json::json;

pub struct Simulator {
    simulation: Arc<Mutex<Simulation>>,
    lua_engine: Arc<Mutex<LuaEngine>>,
    resources: Arc<Mutex<ResourceManager>>,
    waiting_processes: Arc<Mutex<Vec<(String, String)>>>,
    ready_queue: Arc<Mutex<Vec<String>>>,
    waiting_for_time: Arc<Mutex<Vec<(String, f64)>>>, // (process_name, wake_time)
}

impl Simulator {
    pub fn new() -> Self {
        Self {
            simulation: Arc::new(Mutex::new(Simulation::new())),
            lua_engine: Arc::new(Mutex::new(LuaEngine::new())),
            resources: Arc::new(Mutex::new(ResourceManager::new())),
            waiting_processes: Arc::new(Mutex::new(Vec::new())),
            ready_queue: Arc::new(Mutex::new(Vec::new())),
            waiting_for_time: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn load_process(
        &self,
        name: &str,
        script: &str,
        function: &str,
    ) -> Result<(), SimError> {
        let mut engine = self.lua_engine.lock().await;
        engine.create_process(name.to_string(), script, function)?;
        
        // Добавляем процесс в ready_queue
        let mut ready = self.ready_queue.lock().await;
        ready.push(name.to_string());
        
        Ok(())
    }

    pub async fn create_resource(&self, name: &str, capacity: usize) {
        let mut resources = self.resources.lock().await;
        resources.create(name, capacity);
        debug!("Создан ресурс: {} (емкость: {})", name, capacity);
    }

    pub async fn run(&mut self, duration: f64) -> Result<(), SimError> {
        info!("Запуск симуляции на {} секунд", duration);

        let sim = self.simulation.lock().await;
        let start_time = sim.now().await;
        let end_time = SimTime::new(start_time.as_seconds() + duration);
        drop(sim);

        // Основной цикл симуляции
        while self.now().await < end_time {
            // Обновляем время в Lua процессах
            {
                let current_time = self.now().await;
                let mut engine = self.lua_engine.lock().await;
                engine.update_time(current_time.as_seconds());
            }

            // Проверяем процессы, ожидающие времени
            self.check_waiting_for_time().await;

            // Запускаем готовые процессы
            self.run_ready_processes().await?;

            // Обрабатываем сообщения от Lua процессов (ВАЖНО: после run_ready_processes)
            self.process_lua_messages().await?;

            // Проверяем ресурсы
            self.check_waiting_processes().await;

            // Обрабатываем события
            let sim = self.simulation.lock().await;
            let has_events = sim.has_events().await;
            drop(sim);

            if has_events {
                let sim = self.simulation.lock().await;
                sim.process_next_event().await?;
            } else {
                // Проверяем, есть ли активность
                let ready = self.ready_queue.lock().await;
                let has_ready = !ready.is_empty();
                drop(ready);

                let waiting = self.waiting_for_time.lock().await;
                let has_waiting = !waiting.is_empty();
                drop(waiting);

                let waiting_procs = self.waiting_processes.lock().await;
                let has_waiting_procs = !waiting_procs.is_empty();
                drop(waiting_procs);

                // Если нет никакой активности, завершаем симуляцию
                if !has_ready && !has_waiting && !has_waiting_procs {
                    info!("Нет активных процессов, завершаем симуляцию");
                    break;
                }

                if !has_ready {
                    // Продвигаем время к следующему событию ожидания
                    let mut waiting = self.waiting_for_time.lock().await;
                    if !waiting.is_empty() {
                        // Сортируем по времени пробуждения
                        waiting.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                        let next_time = waiting[0].1;
                        drop(waiting);
                        
                        // Устанавливаем время симуляции
                        let sim = self.simulation.lock().await;
                        sim.set_time(SimTime::new(next_time)).await;
                    } else {
                        drop(waiting);
                        tokio::task::yield_now().await;
                    }
                } else {
                    tokio::task::yield_now().await;
                }
            }
        }

        info!("Симуляция завершена. Время: {}", self.now().await);
        Ok(())
    }

    async fn run_ready_processes(&self) -> Result<(), SimError> {
        let mut ready = self.ready_queue.lock().await;
        let process_names: Vec<String> = ready.drain(..).collect();
        drop(ready);

        let mut engine = self.lua_engine.lock().await;

        for name in process_names.iter() {
            if let Some(process) = engine.get_process_mut(name) {
                match process.resume() {
                    Ok(true) => {
                        // Процесс завершен
                        debug!("Процесс {} завершен", name);
                    }
                    Ok(false) => {
                        // Процесс приостановлен (yield) - не добавляем обратно в ready_queue
                        // Он будет добавлен позже, когда условие ожидания выполнится
                        debug!("Процесс {} приостановлен", name);
                    }
                    Err(e) => {
                        error!("Ошибка в процессе {}: {}", name, e);
                    }
                }
            }
        }

        Ok(())
    }

    async fn check_waiting_for_time(&self) {
        let current_time = self.now().await.as_seconds();
        let mut waiting = self.waiting_for_time.lock().await;
        let mut ready = self.ready_queue.lock().await;
        let mut to_remove = Vec::new();

        for (i, (name, wake_time)) in waiting.iter().enumerate() {
            if current_time >= *wake_time {
                debug!("Процесс {} пробужден (время: {})", name, current_time);
                ready.push(name.clone());
                to_remove.push(i);
            }
        }

        for i in to_remove.into_iter().rev() {
            waiting.remove(i);
        }
    }

    async fn now(&self) -> SimTime {
        let sim = self.simulation.lock().await;
        sim.now().await
    }

    async fn process_lua_messages(&self) -> Result<(), SimError> {
        let mut engine = self.lua_engine.lock().await;
        let messages = engine.process_messages().await;
        drop(engine);

        for (process_name, message) in messages {
            match message {
                ProcessMessage::Wait(seconds) => {
                    debug!("Процесс {} ждет {} сек", process_name, seconds);

                    let mut engine = self.lua_engine.lock().await;
                    engine.set_process_waiting(&process_name, seconds);
                    drop(engine);

                    // Вычисляем время пробуждения
                    let current_time = self.now().await.as_seconds();
                    let wake_time = current_time + seconds;

                    // Добавляем в список ожидающих
                    let mut waiting = self.waiting_for_time.lock().await;
                    waiting.push((process_name.clone(), wake_time));
                    
                    debug!("Процесс {} будет пробужден в {}", process_name, wake_time);
                }

                ProcessMessage::Request(resource) => {
                    debug!("Процесс {} запрашивает ресурс {}", process_name, resource);

                    let mut resources = self.resources.lock().await;
                    if resources.request(&resource) {
                        drop(resources);
                        // Ресурс получен немедленно
                        let mut engine = self.lua_engine.lock().await;
                        engine.send_command(&process_name, LuaCommand::ResourceGranted(resource))
                            .map_err(|e| SimError::ProcessError(e))?;
                    } else {
                        drop(resources);
                        let mut engine = self.lua_engine.lock().await;
                        engine.set_process_waiting_for_resource(&process_name, resource.clone());
                        drop(engine);
                        let mut waiting = self.waiting_processes.lock().await;
                        waiting.push((process_name.clone(), resource.clone()));
                        debug!("Процесс {} встал в очередь к {}", process_name, resource);
                    }
                }

                ProcessMessage::Release(resource) => {
                    debug!("Процесс {} освобождает ресурс {}", process_name, resource);

                    let mut resources = self.resources.lock().await;
                    resources.release(&resource);
                }

                ProcessMessage::Log(message, level) => {
                    match level {
                        LogLevel::Info => info!("[{}] {}", process_name, message),
                        LogLevel::Warning => warn!("[{}] {}", process_name, message),
                        LogLevel::Error => error!("[{}] {}", process_name, message),
                        LogLevel::Debug => debug!("[{}] {}", process_name, message),
                    }
                }

                ProcessMessage::Finished => {
                    info!("Процесс {} завершен", process_name);
                }

                ProcessMessage::Spawn(name, func) => {
                    info!("Процесс {} создает новый процесс {} (функция: {})", process_name, name, func);
                    
                    let mut engine = self.lua_engine.lock().await;
                    match engine.spawn_process(name.clone(), &func) {
                        Ok(()) => {
                            // Обновляем время в новом процессе
                            let current_time = self.now().await;
                            engine.update_time(current_time.as_seconds());
                            
                            // Добавляем в ready_queue
                            drop(engine);
                            let mut ready = self.ready_queue.lock().await;
                            ready.push(name.clone());
                            
                            info!("Процесс {} добавлен в ready_queue", name);
                        }
                        Err(e) => {
                            error!("Не удалось создать процесс {}: {}", name, e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn check_waiting_processes(&self) {
        let mut waiting = self.waiting_processes.lock().await;
        let mut to_remove = Vec::new();

        for (i, (process_name, resource_name)) in waiting.iter().enumerate() {
            let mut resources = self.resources.lock().await;
            if resources.request(resource_name) {
                debug!("Ресурс {} доступен для {}", resource_name, process_name);
                to_remove.push(i);

                let mut engine = self.lua_engine.lock().await;
                let _ = engine.send_command(process_name, LuaCommand::ResourceGranted(resource_name.clone()));
                drop(engine);
                
                // Добавляем процесс в ready_queue
                let mut ready = self.ready_queue.lock().await;
                ready.push(process_name.clone());
            }
        }

        for i in to_remove.into_iter().rev() {
            waiting.remove(i);
        }
    }

    pub async fn get_stats(&self) -> serde_json::Value {
        let resources = self.resources.lock().await;
        let engine = self.lua_engine.lock().await;

        json!({
            "time": self.now().await.as_seconds(),
            "active_processes": engine.active_processes().len(),
            "resources": resources.get_stats(),
        })
    }
}

impl Default for Simulator {
    fn default() -> Self {
        Self::new()
    }
}
