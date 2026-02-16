use simpy_rs::Simulator;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Инициализируем логирование
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🏦 Симуляция банка");
    println!("==================\n");

    // Создаем симулятор
    let mut sim = Simulator::new();

    // Создаем ресурсы
    sim.create_resource("кассир", 2).await;
    sim.create_resource("банкомат", 3).await;

    // Загружаем скрипт клиента
    let client_script = r#"
        function client()
            log("Клиент пришел в банк в " .. now() .. " сек", "info")

            -- Выбираем случайный тип обслуживания
            local service_type = math.random(1, 2)

            if service_type == 1 then
                log("Иду к кассиру", "debug")
                request("кассир")
                log("Получил кассира, обслуживаюсь", "info")
                wait(math.random(3, 7))  -- обслуживание 3-7 секунд
                release("кассир")
            else
                log("Иду к банкомату", "debug")
                request("банкомат")
                log("Получил банкомат, обслуживаюсь", "info")
                wait(math.random(1, 3))  -- обслуживание 1-3 секунды
                release("банкомат")
            end

            log("Клиент обслужен и уходит в " .. now() .. " сек", "info")
        end
    "#;

    // Загружаем скрипт генератора (создает клиентов через spawn)
    let generator_script = r#"
        function generator()
            log("Генератор запущен", "info")
            
            -- Создаем 5 клиентов с интервалами
            for i = 1, 5 do
                wait(math.random(2, 5))  -- ждем 2-5 секунд
                log("Создаю клиента " .. i, "info")
                spawn("client_" .. i, "client")
            end
            
            log("Генератор завершил работу", "info")
        end
    "#;

    // Загружаем процессы
    sim.load_process("client", client_script, "client").await?;
    sim.load_process("generator", generator_script, "generator").await?;

    // Запускаем симуляцию
    sim.run(60.0).await?;  // 60 секунд

    // Выводим статистику
    let stats = sim.get_stats().await;
    println!("\n📊 Статистика симуляции:");
    println!("{}", serde_json::to_string_pretty(&stats)?);

    Ok(())
}
