// Добавим явное указание использовать крейт
extern crate simpy_rs;

use simpy_rs::Simulator;
use tracing_subscriber;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Инициализируем логирование
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("🚀 Запуск тестовой симуляции");

    // Создаем симулятор
    let mut sim = Simulator::new();

    // Создаем ресурс
    sim.create_resource("тестовый_ресурс", 2).await;
    println!("✅ Создан ресурс: тестовый_ресурс (емкость: 2)");

    // Простой процесс, который использует wait
    let wait_script = r#"
        function wait_test()
            print("Процесс wait_test начал работу")
            log("Начинаю ждать 3 секунды", "info")
            wait(3.0)
            log("Ожидание завершено", "info")
            print("Процесс wait_test завершен")
        end
    "#;

    // Процесс, который использует ресурсы
    let resource_script = r#"
        function resource_test()
            print("Процесс resource_test начал работу")

            log("Запрашиваю ресурс", "info")
            request("тестовый_ресурс")

            log("Ресурс получен, работаю...", "info")
            wait(2.0)

            log("Освобождаю ресурс", "info")
            release("тестовый_ресурс")

            print("Процесс resource_test завершен")
        end
    "#;

    // Загружаем процессы
    println!("📝 Загрузка процессов...");
    sim.load_process("wait_test", wait_script, "wait_test").await?;
    sim.load_process("resource_test", resource_script, "resource_test").await?;
    println!("✅ Процессы загружены");

    // Даем время на инициализацию
    sleep(Duration::from_millis(100)).await;

    // Запускаем симуляцию на 10 секунд
    println!("▶️ Запуск симуляции...");
    sim.run(10.0).await?;

    // Получаем статистику
    println!("\n📊 Статистика симуляции:");
    let stats = sim.get_stats().await;
    println!("{}", serde_json::to_string_pretty(&stats)?);

    println!("\n✨ Тест завершен успешно!");
    Ok(())
}
