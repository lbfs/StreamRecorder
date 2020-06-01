mod orchestrator;
mod twitch;
mod configuration;
use orchestrator::StreamOrchestrator;


fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filename = String::from("config.json");
    let mut monitor = StreamOrchestrator::new(filename);
    monitor.run();

    Ok(())
}