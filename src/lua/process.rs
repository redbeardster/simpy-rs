//! Представление Lua-процесса в симуляции

use mlua::{Lua, Result as LuaResult};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use super::api;

/// Сообщения от Lua процесса к ядру симуляции
#[derive(Debug)]
pub enum ProcessMessage {
    Wait(f64),
    Request(String),
    Release(String),
    Finished,
    Spawn(String, String),
    Log(String, LogLevel),
}

#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
    Debug,
}

/// Команды от ядра симуляции к Lua процессу
#[derive(Debug)]
pub enum LuaCommand {
    Resume,
    ResourceGranted(String),
    Error(String),
    Terminate,
}

/// Состояние Lua процесса
#[derive(Debug, PartialEq)]
pub enum ProcessState {
    Active,
    Waiting(f64),
    WaitingForResource(String),
    Finished,
}

/// Представляет один процесс, написанный на Lua
pub struct LuaProcess {
    name: String,
    lua: Lua,
    coroutine_key: mlua::RegistryKey,
    state: ProcessState,
    tx: mpsc::UnboundedSender<ProcessMessage>,
}

impl LuaProcess {
    pub fn new(
        name: String,
        script_content: &str,
        function_name: &str,
    ) -> LuaResult<(Self, mpsc::UnboundedReceiver<ProcessMessage>)> {
        let (process_tx, process_rx) = mpsc::unbounded_channel();

        // Создаём Lua
        let lua = Lua::new();
        
        // Устанавливаем имя процесса в глобальной переменной
        {
            let globals = lua.globals();
            globals.set("_process_name", name.clone())?;
            globals.set("_current_time", 0.0)?;
        }
        
        // Регистрируем API
        api::register_api(&lua, process_tx.clone())?;

        // Загружаем скрипт
        lua.load(script_content).exec()?;

        // Создаем корутину из функции и сохраняем в registry
        let coroutine_key = {
            let globals = lua.globals();
            let func: mlua::Function = globals.get(function_name)?;
            let thread = lua.create_thread(func)?;
            lua.create_registry_value(thread)?
        };

        Ok((
            Self {
                name,
                lua,
                coroutine_key,
                state: ProcessState::Active,
                tx: process_tx,
            },
            process_rx,
        ))
    }

    /// Возобновляет выполнение корутины
    /// Возвращает:
    /// - Ok(true) - корутина завершена
    /// - Ok(false) - корутина приостановлена (yield)
    /// - Err(e) - ошибка выполнения
    pub fn resume(&mut self) -> LuaResult<bool> {
        if self.state == ProcessState::Finished {
            return Ok(true);
        }

        let coroutine: mlua::Thread = self.lua.registry_value(&self.coroutine_key)?;
        let status = coroutine.status();
        
        match status {
            mlua::ThreadStatus::Resumable => {
                // Пытаемся возобновить корутину
                match coroutine.resume::<_, mlua::Value>(()) {
                    Ok(_) => {
                        // Проверяем новый статус
                        let new_status = coroutine.status();
                        match new_status {
                            mlua::ThreadStatus::Resumable => {
                                // Корутина приостановлена (yield)
                                debug!("Процесс {} приостановлен", self.name);
                                Ok(false)
                            }
                            mlua::ThreadStatus::Unresumable => {
                                // Корутина завершилась
                                self.state = ProcessState::Finished;
                                let _ = self.tx.send(ProcessMessage::Finished);
                                info!("Процесс {} завершен", self.name);
                                Ok(true)
                            }
                            mlua::ThreadStatus::Error => {
                                error!("Процесс {} завершился с ошибкой", self.name);
                                self.state = ProcessState::Finished;
                                Ok(true)
                            }
                        }
                    }
                    Err(e) => {
                        error!("Ошибка в процессе {}: {}", self.name, e);
                        self.state = ProcessState::Finished;
                        Err(e)
                    }
                }
            }
            mlua::ThreadStatus::Unresumable => {
                // Корутина уже завершена
                self.state = ProcessState::Finished;
                Ok(true)
            }
            mlua::ThreadStatus::Error => {
                error!("Процесс {} в состоянии ошибки", self.name);
                self.state = ProcessState::Finished;
                Ok(true)
            }
        }
    }

    pub fn state(&self) -> &ProcessState {
        &self.state
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_waiting(&mut self, duration: f64) {
        self.state = ProcessState::Waiting(duration);
    }

    pub fn set_waiting_for_resource(&mut self, resource: String) {
        self.state = ProcessState::WaitingForResource(resource);
    }

    pub fn set_active(&mut self) {
        self.state = ProcessState::Active;
    }

    pub fn terminate(&mut self) {
        self.state = ProcessState::Finished;
    }

    pub fn update_time(&mut self, time: f64) -> LuaResult<()> {
        let globals = self.lua.globals();
        globals.set("_current_time", time)?;
        Ok(())
    }
}
