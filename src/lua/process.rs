//! Представление Lua-процесса в симуляции

use mlua::{Lua, Result as LuaResult, RegistryKey};
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
    process_func: RegistryKey,
    state: ProcessState,
    tx: mpsc::UnboundedSender<ProcessMessage>,
    pending_commands: Vec<LuaCommand>,
}

impl LuaProcess {
    pub fn new(
        name: String,
        script_content: &str,
        function_name: &str,
    ) -> LuaResult<(Self, mpsc::UnboundedReceiver<ProcessMessage>)> {
        let (process_tx, process_rx) = mpsc::unbounded_channel();

        // Создаём Lua и регистрируем API
        let lua = Lua::new();
        api::register_api(&lua, process_tx.clone())?;

        // Загружаем скрипт
        lua.load(script_content).exec()?;

        // Получаем функцию процесса и сохраняем в реестр
        let registry_key = {
            let globals = lua.globals();
            let func: mlua::Function = globals.get(function_name)?;
            lua.create_registry_value(func)?
        };

        Ok((
            Self {
                name,
                lua,
                process_func: registry_key,
                state: ProcessState::Active,
                tx: process_tx,
                pending_commands: Vec::new(),
            },
            process_rx,
        ))
    }

    pub async fn resume(&mut self) -> LuaResult<()> {
        if self.state == ProcessState::Finished {
            debug!("Процесс {} уже завершен", self.name);
            return Ok(());
        }

        // Обрабатываем ожидающие команды
        for cmd in self.pending_commands.drain(..) {
            match cmd {
                LuaCommand::ResourceGranted(resource) => {
                    let globals = self.lua.globals();
                    let _ = globals.set("_resource_granted", resource);
                }
                LuaCommand::Error(e) => {
                    let globals = self.lua.globals();
                    let _ = globals.set("_error", e);
                }
                LuaCommand::Terminate => {
                    self.state = ProcessState::Finished;
                    return Ok(());
                }
                LuaCommand::Resume => {}
            }
        }

        let func: mlua::Function = self.lua.registry_value(&self.process_func)?;

        // Запускаем функцию
        match func.call_async::<_, ()>(()).await {
            Ok(()) => {
                self.state = ProcessState::Finished;
                let _ = self.tx.send(ProcessMessage::Finished);
                info!("Процесс {} завершен", self.name);
            }
            Err(e) => {
                error!("Ошибка в процессе {}: {}", self.name, e);
                self.state = ProcessState::Finished;
                return Err(e);
            }
        }

        Ok(())
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

    pub fn push_command(&mut self, command: LuaCommand) {
        self.pending_commands.push(command);
    }

    pub fn update_time(&mut self, time: f64) -> LuaResult<()> {
        let globals = self.lua.globals();
        globals.set("_sim_time", time)?;
        Ok(())
    }
}
