use simpy_rs::core::{Simulation, Duration, SimTime, Priority};

#[tokio::test]
async fn test_basic_simulation() {
    let mut sim = Simulation::new();

    // Планируем несколько событий
    let results = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));

    for i in 0..5 {
        let results = results.clone();
        sim.schedule_after(
            Duration::from_seconds(i as f64),
            Priority::Normal,
            move || {
                let mut results = results.try_lock().unwrap();
                results.push(i);
            }
        ).await.unwrap();
    }

    sim.run_for(Duration::from_seconds(10.0)).await.unwrap();

    let final_results = results.lock().await;
    assert_eq!(*final_results, vec![0, 1, 2, 3, 4]);
}
