//! Полноценная симуляция с Lua скриптингом

use crate::core::Simulation;
use crate::lua::{LuaEngine, ProcessMessage, ProcessState, LuaCommand};
use crate::resources::ResourceManager;

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, debug, warn};

/// Основной класс симуляции с поддержкой Lua
pub struct Simulator {
    /// Ядро симуляции
    simulation: Simulation,
    /// Lua движок
    lua_engine: Arc<Mutex<LuaEngine>>,
    /// Менеджер ресурсов
    resources: Arc<Mutex<ResourceManager>>,
    /// Очередь процессов, ожидающих ресурсы
    waiting_processes: Arc<Mutex<Vec<(String, String)>>>, // (process_name, resource_name)
}

impl Simulator {
    pub fn new() -> Self {
        Self {
            simulation: Simulation::new(),
            lua_engine: Arc::new(Mutex::new(LuaEngine::new())),
            resources: Arc::new(Mutex::new(ResourceManager::new())),
            waiting_processes: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Загрузить Lua скрипт с определением процесса
    pub async fn load_process(
        &self,
        name: &str,
        script: &str,
        function: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut engine = self.lua_engine.lock().await;
        engine.create_process(name.to_string(), script, function)?;
        Ok(())
    }

    /// Создать ресурс
    pub async fn create_resource(&self, name: &str, capacity: usize) {
        let mut resources = self.resources.lock().await;
        resources.create(name, capacity);
    }

    /// Запустить симуляцию
    pub async fn run(&mut self, duration: f64) -> Result<(), Box<dyn std::error::Error>> {
        info!("Запуск симуляции на {} секунд", duration);

        // Запускаем все процессы
        {
            let mut engine = self.lua_engine.lock().await;
            for process_name in engine.active_processes() {
                if let Err(e) = engine.start_process(process_name).await {
                    warn!("Не удалось запустить процесс {}: {}", process_name, e);
                }
            }
        }

        // Основной цикл симуляции
        let start_time = self.simulation.now().await;
        let end_time = crate::core::SimTime::new(start_time.as_seconds() + duration);

        while self.simulation.now().await < end_time {
            // Обрабатываем сообщения от Lua процессов
            self.process_lua_messages().await?;

            // Проверяем ожидающие ресурсы
            self.check_waiting_processes().await;

            // Делаем шаг симуляции
            if self.simulation.has_events().await {
                self.simulation.process_next_event().await?;
            } else {
                // Нет событий - выходим
                break;
            }
        }

        info!("Симуляция завершена. Время: {}", self.simulation.now().await);
        Ok(())
    }

    /// Обработка сообщений от Lua процессов
    async fn process_lua_messages(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut engine = self.lua_engine.lock().await;
        let messages = engine.process_messages().await;

        for (process_name, message) in messages {
            match message {
                ProcessMessage::Wait(seconds) => {
                    debug!("Процесс {} ждет {} сек", process_name, seconds);

                    // Планируем возобновление процесса
                    let engine_clone = self.lua_engine.clone();
                    self.simulation.schedule_after(
                        crate::core::Duration::from_seconds(seconds),
                        crate::core::Priority::Normal,
                        move || {
                            // Возобновляем процесс
                            tokio::spawn(async move {
                                let mut engine = engine_clone.lock().await;
                                let _ = engine.start_process(&process_name).await;
                            });
                        },
                    ).await?;
                }

                ProcessMessage::Request(resource) => {
                    debug!("Процесс {} запрашивает ресурс {}", process_name, resource);

                    let mut resources = self.resources.lock().await;
                    if resources.request(&resource) {
                        // Ресурс получен немедленно
                        let engine_clone = self.lua_engine.clone();
                        tokio::spawn(async move {
                            let mut engine = engine_clone.lock().await;
                            let _ = engine.send_command(&process_name, LuaCommand::ResourceGranted(resource));
                        });
                    } else {
                        // Ресурс занят, встаем в очередь
                        drop(resources); // Освобождаем блокировку
                        let mut waiting = self.waiting_processes.lock().await;
                        waiting.push((process_name, resource));
                    }
                }

                ProcessMessage::Release(resource) => {
                    debug!("Процесс {} освобождает ресурс {}", process_name, resource);

                    let mut resources = self.resources.lock().await;
                    resources.release(&resource);
                }

                ProcessMessage::Log(message, level) => {
                    match level {
                        crate::lua::LogLevel::Info => info!("[{}] {}", process_name, message),
                        crate::lua::LogLevel::Warning => warn!("[{}] {}", process_name, message),
                        crate::lua::LogLevel::Error => error!("[{}] {}", process_name, message),
                        crate::lua::LogLevel::Debug => debug!("[{}] {}", process_name, message),
                    }
                }

                ProcessMessage::Finished => {
                    info!("Процесс {} завершен", process_name);
                }

                ProcessMessage::Spawn(name, func) => {
                    info!("Процесс {} создает новый процесс {}", process_name, name);
                    // TODO: реализовать создание процессов из Lua
                }
            }
        }

        Ok(())
    }

    /// Проверка процессов, ожидающих ресурсы
    async fn check_waiting_processes(&self) {
        let mut waiting = self.waiting_processes.lock().await;
        let mut to_remove = Vec::new();

        for (i, (process_name, resource_name)) in waiting.iter().enumerate() {
            let mut resources = self.resources.lock().await;
            if resources.request(resource_name) {
                // Ресурс освободился
                to_remove.push(i);

                let engine_clone = self.lua_engine.clone();
                let process_name = process_name.clone();
                let resource_name = resource_name.clone();

                tokio::spawn(async move {
                    let mut engine = engine_clone.lock().await;
                    let _ = engine.send_command(&process_name, LuaCommand::ResourceGranted(resource_name));
                });
            }
        }

        // Удаляем обработанные запросы (в обратном порядке)
        for i in to_remove.into_iter().rev() {
            waiting.remove(i);
        }
    }

    /// Получить текущую статистику
    pub async fn get_stats(&self) -> serde_json::Value {
        let resources = self.resources.lock().await;
        let engine = self.lua_engine.lock().await;

        serde_json::json!({
            "time": self.simulation.now().await.as_seconds(),
            "active_processes": engine.active_processes().len(),
            "resources": resources.get_stats(),
        })
    }
}
