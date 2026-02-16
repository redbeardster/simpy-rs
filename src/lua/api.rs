//! API функции для Lua

use mlua::{Lua, Result, Value};
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::debug;

use super::process::{ProcessMessage, LogLevel};

/// Регистрация API функций в Lua
pub fn register_api(
    lua: &Lua,
    tx: mpsc::UnboundedSender<ProcessMessage>,
    _wakeup_tx: mpsc::UnboundedSender<oneshot::Sender<()>>,
) -> Result<()> {
    let globals = lua.globals();

    // Инициализируем переменную времени
    globals.set("_sim_time", 0.0)?;

    // now() - получить текущее время симуляции
    let now_fn = lua.create_function(|lua, ()| {
        let globals = lua.globals();
        let time: f64 = globals.get("_sim_time")?;
        Ok(time)
    })?;
    globals.set("now", now_fn)?;

    // wait(seconds)
    let tx_wait = tx.clone();
    let wait_fn = lua.create_async_function(move |_lua, seconds: f64| {
        let tx_wait = tx_wait.clone();
        async move {
            if seconds < 0.0 {
                return Err(mlua::Error::external("wait time cannot be negative"));
            }

            // Создаем канал для пробуждения
            let (wakeup_tx, wakeup_rx) = oneshot::channel();

            // Отправляем сообщение с каналом пробуждения
            tx_wait.send(ProcessMessage::Wait(seconds, wakeup_tx))
                .map_err(|e| mlua::Error::external(format!("failed to send wait: {}", e)))?;

            // Ждем сигнала пробуждения
            wakeup_rx.await
                .map_err(|e| mlua::Error::external(format!("wait interrupted: {}", e)))?;

            Ok(Value::Nil)
        }
    })?;
    globals.set("wait", wait_fn)?;

    // request(resource)
    let tx_request = tx.clone();
    let request_fn = lua.create_function(move |_, resource: String| {
        tx_request.send(ProcessMessage::Request(resource))
            .map_err(|e| mlua::Error::external(format!("failed to send request: {}", e)))?;
        Ok(Value::Nil)
    })?;
    globals.set("request", request_fn)?;

    // release(resource)
    let tx_release = tx.clone();
    let release_fn = lua.create_function(move |_, resource: String| {
        tx_release.send(ProcessMessage::Release(resource))
            .map_err(|e| mlua::Error::external(format!("failed to send release: {}", e)))?;
        Ok(Value::Nil)
    })?;
    globals.set("release", release_fn)?;

    // log(message, level)
    let tx_log = tx.clone();
    let log_fn = lua.create_function(move |_, (message, level): (String, Option<String>)| {
        let log_level = match level.as_deref() {
            Some("warning") | Some("warn") => LogLevel::Warning,
            Some("error") => LogLevel::Error,
            Some("debug") => LogLevel::Debug,
            _ => LogLevel::Info,
        };

        tx_log.send(ProcessMessage::Log(message, log_level))
            .map_err(|e| mlua::Error::external(format!("failed to send log: {}", e)))?;

        Ok(())
    })?;
    globals.set("log", log_fn)?;

    // spawn(name, function_name)
    let tx_spawn = tx.clone();
    let spawn_fn = lua.create_function(move |_, (name, func_name): (String, String)| {
        tx_spawn.send(ProcessMessage::Spawn(name, func_name))
            .map_err(|e| mlua::Error::external(format!("failed to spawn: {}", e)))?;
        Ok(())
    })?;
    globals.set("spawn", spawn_fn)?;

    debug!("Lua API functions registered");

    Ok(())
}
