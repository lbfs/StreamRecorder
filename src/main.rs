mod monitor;
mod twitch;

use monitor::Monitor;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filename = String::from("config.json");
    let mut monitor = Monitor::new(filename);
    monitor.run();

    Ok(())
}