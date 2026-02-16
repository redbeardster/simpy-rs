//! Полноценная симуляция с Lua скриптингом

use crate::core::{Simulation, Duration, Priority, SimTime};
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
    resume_queue: Arc<Mutex<Vec<String>>>,
}

impl Simulator {
    pub fn new() -> Self {
        Self {
            simulation: Arc::new(Mutex::new(Simulation::new())),
            lua_engine: Arc::new(Mutex::new(LuaEngine::new())),
            resources: Arc::new(Mutex::new(ResourceManager::new())),
            waiting_processes: Arc::new(Mutex::new(Vec::new())),
            resume_queue: Arc::new(Mutex::new(Vec::new())),
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

        // Создаем LocalSet для запуска !Send задач
        let local = tokio::task::LocalSet::new();
        
        // Запускаем все процессы в LocalSet
        {
            let engine = self.lua_engine.clone();
            let processes: Vec<String> = {
                let eng = engine.lock().await;
                eng.active_processes()
            };
            
            for name in processes {
                let engine_clone = engine.clone();
                local.spawn_local(async move {
                    let mut eng = engine_clone.lock().await;
                    if let Err(e) = eng.start_process(&name).await {
                        warn!("Не удалось запустить процесс {}: {}", name, e);
                    }
                });
            }
        }

        // Запускаем основной цикл симуляции внутри LocalSet
        local.run_until(async {
            while self.now().await < end_time {
                // Обновляем время в Lua процессах
                {
                    let current_time = self.now().await;
                    let mut engine = self.lua_engine.lock().await;
                    engine.update_time(current_time.as_seconds());
                }

                let sim = self.simulation.lock().await;
                let has_events = sim.has_events().await;
                drop(sim);

                if has_events {
                    let sim = self.simulation.lock().await;
                    sim.process_next_event().await?;
                } else {
                    // Даем возможность выполниться другим задачам
                    tokio::task::yield_now().await;
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }

            info!("Симуляция завершена. Время: {}", self.now().await);
            Ok::<(), SimError>(())
        }).await?;

        Ok(())
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
                ProcessMessage::Wait(seconds, wakeup_sender) => {
                    debug!("Процесс {} ждет {} сек", process_name, seconds);

                    let mut engine = self.lua_engine.lock().await;
                    engine.set_process_waiting(&process_name, seconds);
                    drop(engine);

                    let proc_name = process_name.clone();
                    let sim = self.simulation.lock().await;
                    sim.schedule_after(
                        Duration::from_seconds(seconds),
                        Priority::Normal,
                        move || {
                            // Отправляем сигнал пробуждения
                            if wakeup_sender.send(()).is_err() {
                                error!("Не удалось разбудить процесс {}: получатель отпал", proc_name);
                            }
                            debug!("Процесс {} пробужден", proc_name);
                        },
                    ).await?;
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
                            
                            // Запускаем новый процесс
                            if let Err(e) = engine.start_process(&name).await {
                                error!("Не удалось запустить созданный процесс {}: {}", name, e);
                            }
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
            }
        }

        for i in to_remove.into_iter().rev() {
            waiting.remove(i);
        }
    }

    async fn process_resume_queue(&self) -> Result<(), SimError> {
        let mut queue = self.resume_queue.lock().await;
        let to_resume: Vec<String> = queue.drain(..).collect();
        drop(queue);

        for proc_name in to_resume {
            let mut engine = self.lua_engine.lock().await;
            engine.set_process_active(&proc_name);
            if let Err(e) = engine.start_process(&proc_name).await {
                error!("Ошибка при возобновлении {}: {}", proc_name, e);
            }
        }

        Ok(())
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
