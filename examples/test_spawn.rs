use simpy_rs::Simulator;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Инициализируем логирование
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🧪 Тест функции spawn");
    println!("=====================\n");

    let mut sim = Simulator::new();

    // Скрипт дочернего процесса
    let child_script = r#"
        function child()
            log("Дочерний процесс запущен в " .. now() .. " сек", "info")
            wait(2)
            log("Дочерний процесс завершен в " .. now() .. " сек", "info")
        end
    "#;

    // Скрипт родительского процесса
    let parent_script = r#"
        function parent()
            log("Родительский процесс начал работу в " .. now() .. " сек", "info")
            wait(1)
            
            log("Создаю дочерний процесс 1", "info")
            spawn("child_1", "child")
            
            wait(1)
            
            log("Создаю дочерний процесс 2", "info")
            spawn("child_2", "child")
            
            wait(1)
            
            log("Родительский процесс завершен в " .. now() .. " сек", "info")
        end
    "#;

    // Загружаем процессы
    sim.load_process("child", child_script, "child").await?;
    sim.load_process("parent", parent_script, "parent").await?;

    println!("▶️  Запуск симуляции...\n");
    
    // Запускаем симуляцию в LocalSet
    let local = tokio::task::LocalSet::new();
    local.run_until(async {
        sim.run(10.0).await?;

        // Выводим статистику
        let stats = sim.get_stats().await;
        println!("\n📊 Статистика:");
        println!("   Время: {} сек", stats["time"]);
        println!("   Активных процессов: {}", stats["active_processes"]);
        
        println!("\n✨ Тест завершен успешно!");

        Ok::<(), Box<dyn std::error::Error>>(())
    }).await?;

    Ok(())
}
