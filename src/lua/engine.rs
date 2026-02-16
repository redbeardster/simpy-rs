//! Движок для управления Lua процессами

use mlua::Result as LuaResult;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{info, debug};

use super::process::{LuaProcess, ProcessMessage, ProcessState, LuaCommand};

pub struct LuaEngine {
    processes: HashMap<String, LuaProcess>,
    process_receivers: HashMap<String, mpsc::UnboundedReceiver<ProcessMessage>>,
    scripts: HashMap<String, String>, // Храним загруженные скрипты
}

impl LuaEngine {
    pub fn new() -> Self {
        Self {
            processes: HashMap::new(),
            process_receivers: HashMap::new(),
            scripts: HashMap::new(),
        }
    }

    pub fn create_process(
        &mut self,
        name: String,
        script_content: &str,
        function_name: &str,
    ) -> LuaResult<()> {
        if self.processes.contains_key(&name) {
            return Err(mlua::Error::external(format!(
                "Process with name '{}' already exists",
                name
            )));
        }

        let (process, receiver) = LuaProcess::new(
            name.clone(),
            script_content,
            function_name,
        )?;

        self.processes.insert(name.clone(), process);
        self.process_receivers.insert(name.clone(), receiver);
        
        // Сохраняем скрипт для возможности создания новых процессов
        if !self.scripts.contains_key(function_name) {
            self.scripts.insert(function_name.to_string(), script_content.to_string());
        }

        info!("Создан процесс: {}", name);
        Ok(())
    }

    pub fn spawn_process(
        &mut self,
        name: String,
        function_name: &str,
    ) -> Result<(), String> {
        if self.processes.contains_key(&name) {
            return Err(format!("Process with name '{}' already exists", name));
        }

        // Ищем скрипт по имени функции
        let script_content = self.scripts.get(function_name)
            .ok_or_else(|| format!("Function '{}' not found in loaded scripts", function_name))?
            .clone();

        let (process, receiver) = LuaProcess::new(
            name.clone(),
            &script_content,
            function_name,
        ).map_err(|e| format!("Failed to create process: {}", e))?;

        self.processes.insert(name.clone(), process);
        self.process_receivers.insert(name.clone(), receiver);

        info!("Создан процесс через spawn: {}", name);
        Ok(())
    }

    pub async fn start_process(&mut self, name: &str) -> LuaResult<()> {
        if let Some(process) = self.processes.get_mut(name) {
            process.resume().await
        } else {
            Err(mlua::Error::external(format!(
                "Process '{}' not found",
                name
            )))
        }
    }

    pub async fn process_messages(&mut self) -> Vec<(String, ProcessMessage)> {
        let mut messages = Vec::new();

        for (name, receiver) in self.process_receivers.iter_mut() {
            while let Ok(msg) = receiver.try_recv() {
                debug!("Сообщение от {}: {:?}", name, msg);
                messages.push((name.clone(), msg));
            }
        }

        messages
    }

    pub fn cleanup_finished(&mut self) {
        let finished: Vec<String> = self.processes
            .iter()
            .filter(|(_, p)| matches!(p.state(), ProcessState::Finished))
            .map(|(name, _)| name.clone())
            .collect();

        for name in finished {
            self.processes.remove(&name);
            self.process_receivers.remove(&name);
            info!("Процесс {} удален", name);
        }
    }

    pub fn send_command(&mut self, process_name: &str, command: LuaCommand) -> Result<(), String> {
        if let Some(process) = self.processes.get_mut(process_name) {
            process.push_command(command);
            Ok(())
        } else {
            Err(format!("Process '{}' not found", process_name))
        }
    }

    pub fn active_processes(&self) -> Vec<String> {
        self.processes.keys().cloned().collect()
    }

    pub fn process_state(&self, name: &str) -> Option<&ProcessState> {
        self.processes.get(name).map(|p| p.state())
    }

    pub fn set_process_waiting(&mut self, name: &str, duration: f64) {
        if let Some(process) = self.processes.get_mut(name) {
            process.set_waiting(duration);
        }
    }

    pub fn set_process_waiting_for_resource(&mut self, name: &str, resource: String) {
        if let Some(process) = self.processes.get_mut(name) {
            process.set_waiting_for_resource(resource);
        }
    }

    pub fn set_process_active(&mut self, name: &str) {
        if let Some(process) = self.processes.get_mut(name) {
            process.set_active();
        }
    }

    pub fn terminate_all(&mut self) {
        self.processes.clear();
        self.process_receivers.clear();
    }

    pub fn update_time(&mut self, time: f64) {
        for process in self.processes.values_mut() {
            let _ = process.update_time(time);
        }
    }
}

impl Default for LuaEngine {
    fn default() -> Self {
        Self::new()
    }
}
