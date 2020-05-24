use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::process::{Child, Command, Stdio};
use std::{fs, path, thread, time};

use super::twitch::{TwitchHelixAPI, TwitchStream, TwitchUser};

fn create_recorder_process(unit: &MonitorUnit) -> Command {
    let mut command = Command::new("streamlink");
    command
        .args(&[
            "--twitch-disable-hosting",
            unit.url.as_str(),
            "best",
            "-o",
            path::Path::new(&unit.recording_path).join(&unit.filename).to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn create_cleanup_process(unit: &MonitorUnit) -> Command {
    let mut command = Command::new("ffmpeg");
    command
        .args(&[
            "-nostdin",
            "-y",
            "-err_detect",
            "ignore_err",
            "-i",
            path::Path::new(&unit.recording_path).join(&unit.filename).to_str().unwrap(),
            "-c",
            "copy",
            path::Path::new(&unit.cleanup_path).join(&unit.filename).to_str().unwrap(),
        ]);
    command
}

#[cfg(target_os = "windows")]
fn create_movement_process(unit: &MonitorUnit) -> Command {
    // xcopy /j: Copies files without buffering. Recommended for very large files. This parameter was added in Windows Server 2008 R2.

    let mut command = Command::new("xcopy");
    command
        .args(&[
            path::Path::new(&unit.cleanup_path).join(&unit.filename).to_str().unwrap(),
            path::Path::new(&unit.move_path).to_str().unwrap(),
            "/j"
        ]);
    command
}    

#[cfg(not(target_os = "windows"))]
fn create_movement_process(unit: &MonitorUnit) -> Command {
    let mut command = Command::new("cp");
    command
        .args(&[
            path::Path::new(&unit.cleanup_path).join(&unit.filename).to_str().unwrap(),
            path::Path::new(&unit.move_path).join(&unit.filename).to_str().unwrap(),
        ]);
    command
}

#[derive(Serialize, Deserialize)]
struct Configuration {
    client_id: String,
    client_secret: String,
    login_names: Vec<String>,
    recording_path: path::PathBuf,
    cleanup_path: path::PathBuf,
    move_path: path::PathBuf,
    halt_until_next_live: bool,
    halt_newly_added: bool,
}

enum MonitorStage {
    Start,
    Recording,
    Cleanup,
    Moving,
}

struct MonitorUnit {
    recording_path: path::PathBuf,
    cleanup_path: path::PathBuf,
    move_path: path::PathBuf,
    filename: String,
    url: String,
    stage: MonitorStage,
    child: Option<Child>,
    stream: TwitchStream,
}

pub struct Monitor {
    api: TwitchHelixAPI,
    filename: String,
    config: Configuration,
    modified: std::time::SystemTime,
    refresh_duration: time::Duration,
    previous_length: usize,
    users: HashMap<String, TwitchUser>,
    streams: HashMap<String, MonitorUnit>,
    completed: Vec<String>,
    halt_streams: Vec<TwitchStream>,
}

impl Monitor {
    pub fn new(filename: String) -> Monitor {
        let config = Monitor::load_configuration(&filename);
        let refresh_duration = time::Duration::from_secs(15);
        let api = TwitchHelixAPI::new(config.client_id.clone(), config.client_secret.clone());
        let modified = fs::metadata(&filename).unwrap().modified().unwrap();

        Monitor {
            api: api,
            filename: filename,
            config: config,
            refresh_duration: refresh_duration,
            previous_length: 0,
            modified: modified,
            users: HashMap::new(),
            streams: HashMap::new(),
            completed: Vec::new(),
            halt_streams: Vec::new(),
        }
    }

    pub fn run(&mut self) {
        let local_users = self.api.retrieve_users(&self.config.login_names).unwrap();
        if self.config.halt_until_next_live {
            match self.api.retrieve_streams(local_users.iter()) {
                Ok(streams) => {
                    let extension: Vec<TwitchStream> = streams.into_iter().filter(|stream| !self.streams.contains_key(&stream.id)).collect();
                    self.halt_streams.extend(extension);
                }
                Err(_) => {
                    eprintln!("Failed to retrieve stream information. Attempting to update on next iteration.");
                }
            }
        }

        self.users.clear();
        for user in local_users {
            self.users.insert(user.id.clone(), user);
        }

        loop {
            if self.needs_reload() {
                println!("Attempting to reload the updated configuration.");
                let local_config = Monitor::load_configuration(&self.filename);

                if let Ok(users) = self.api.retrieve_users(&local_config.login_names) {
                    if local_config.halt_newly_added {
                        match self.api.retrieve_streams(users.iter()) {
                            Ok(streams) => {
                                let extension: Vec<TwitchStream> = streams.into_iter().filter(|stream| !self.streams.contains_key(&stream.id)).collect();
                                self.halt_streams.extend(extension);
                            }
                            Err(_) => {
                                eprintln!("Failed to retrieve stream information. Attempting to update on next iteration.");
                            }
                        }
                    }
                    self.config = local_config;
                    self.modified = fs::metadata(&self.filename).unwrap().modified().unwrap();
                    self.users.clear();
                    for user in users {
                        self.users.insert(user.id.clone(), user);
                    }
                    println!("Successfully reloaded configuration.");
                } else {
                    eprintln!("Failed to retrieve user data. Attempting on next iteration.");
                }

                continue;
            }

            match self.api.retrieve_streams(self.users.values()) {
                Ok(streams) => {
                    self.halt_streams.retain(|stream| streams.contains(stream));
                    let startup_streams: Vec<TwitchStream> = streams
                        .into_iter()
                        .filter(|stream| !self.halt_streams.contains(stream))
                        .filter(|stream| !self.streams.contains_key(&stream.id))
                        .collect();

                    for stream in startup_streams {
                        let login_name = &self.users[&stream.user_id].login;
                        println!("Starting recording of {}'s stream.", login_name);
                        
                        static UNSAFE_CHARACTERS: &'static [char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\n', '\r', '\0'];
                        let now = time::SystemTime::now().duration_since(time::SystemTime::UNIX_EPOCH).unwrap().as_secs();
                        let filename = format!("{} - {} - {} - {}.mp4", login_name.trim(), stream.id.trim(), now, stream.title.trim());
                        let filename = filename.replace(|c: char| UNSAFE_CHARACTERS.contains(&c), "");

                        let subdirectory = login_name.trim();
                        let recording_path = path::Path::new(&self.config.recording_path).join(&subdirectory);
                        let cleanup_path = path::Path::new(&self.config.cleanup_path).join(&subdirectory);
                        let move_path = path::Path::new(&self.config.move_path).join(&subdirectory);

                        fs::create_dir_all(&recording_path).unwrap();
                        fs::create_dir_all(&cleanup_path).unwrap();
                        fs::create_dir_all(&move_path).unwrap();

                        let url = format!("https://www.twitch.tv/{}", login_name);

                        let chain = MonitorUnit {
                            recording_path: recording_path,
                            cleanup_path: cleanup_path,
                            move_path: move_path,
                            filename: filename,
                            url: url,
                            stage: MonitorStage::Start,
                            child: None,
                            stream: stream
                        };

                        self.streams.insert(chain.stream.id.clone(), chain);
                    }
                }
                Err(e) => {
                    eprintln!("Application error: {}", e);
                }
            }

            for (stream_id, unit) in self.streams.iter_mut() {
                if let Some(child) = &mut unit.child {
                    match child.try_wait() {
                        Ok(Some(_status)) => {}, 
                        Ok(None) => continue,
                        Err(e) => {
                            eprintln!("Recoverable application error: {}", e);
                            self.completed.push(stream_id.clone());
                            continue;
                        }
                    }
                }
    
                let (mut command, stage) = match unit.stage {
                    MonitorStage::Start => (create_recorder_process(&unit), MonitorStage::Recording),
                    MonitorStage::Recording => (create_cleanup_process(&unit), MonitorStage::Cleanup),
                    MonitorStage::Cleanup => {
                        let recording_full_path = path::Path::new(&unit.recording_path).join(&unit.filename);
                        if let Err(e) = fs::remove_file(recording_full_path) {
                            eprintln!("Failed to remove the file with error: {}\n", e);
                            self.completed.push(stream_id.clone());
                            continue;
                        }
                        (create_movement_process(&unit), MonitorStage::Moving)
                    },
                    MonitorStage::Moving => {
                        let cleanup_full_path = path::Path::new(&unit.cleanup_path).join(&unit.filename);
                        if let Err(e) = fs::remove_file(cleanup_full_path) {
                            eprintln!("Failed to remove the file with error: {}", e);
                        }
                        self.completed.push(stream_id.clone());
                        continue;
                    }
                };
    
                unit.child = match command.spawn() {
                    Ok(child) => Some(child),
                    Err(e) => panic!("Unrecoverable application error: {}", e)
                };
                unit.stage = stage;
            }

            for stream_id in &self.completed {
                if let Some(unit) = self.streams.remove(stream_id) {
                    println!("{} has successfully been processed!", unit.filename)
                }
            }

            let active_streams = self.streams.values().collect::<Vec<&MonitorUnit>>();
            if self.previous_length != active_streams.len() {
                Monitor::print_active_streams(active_streams.as_slice());
                self.previous_length = active_streams.len();
            }

            thread::sleep(self.refresh_duration);
        }
    }

    fn print_active_streams(streams: &[&MonitorUnit]) {
        match streams {
            [] => println!("Not recording."),
            [single] => println!("Actively recording {}'s stream.", single.stream.user_name),
            [first, second] => println!("Actively recording {}'s and {}'s stream.", first.stream.user_name, second.stream.user_name),
            [start @ .., last] => {
                print!("Actively recording ");
                for entry in start {
                    print!("{}, ", entry.stream.user_name);
                }
                println!("and {}'s streams.",  last.stream.user_name);
            }
        }
    }

    fn needs_reload(&self) -> bool {
        match fs::metadata(&self.filename) {
            Ok(value) => match value.modified() {
                Ok(modified) => {
                    self.modified != modified
                },
                Err(_) => false,
            },
            Err(_) => false,
        }
    }

    fn load_configuration(filename: &String) -> Configuration {
        let mut file = File::open(filename).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        let config: Configuration = serde_json::from_str(&contents).unwrap();
        config
    }
}
