use std::process::{Command, Stdio};
use std::{fs, path, thread, time};

use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

use super::configuration::Configuration;
use super::twitch::{TwitchHelixAPI, TwitchStream, TwitchUser};

struct StreamSubprocess {
    stream: TwitchStream,
    child: std::process::Child,
    recording_path: path::PathBuf,
    cleanup_path: path::PathBuf,
    move_path: path::PathBuf,
    filename: String,
}

pub struct StreamOrchestrator {
    api: TwitchHelixAPI,
    config: Configuration,
    filename: String,
    modified: time::SystemTime,
    halt_streams: Vec<TwitchStream>,
    streams: Vec<StreamSubprocess>,
    users: Vec<TwitchUser>,
}

impl StreamOrchestrator {
    pub fn new(filename: String) -> StreamOrchestrator {
        let config = Configuration::new(&filename).unwrap();
        let modified = fs::metadata(&filename).unwrap().modified().unwrap();

        let mut api = TwitchHelixAPI::new(config.client_id.clone(), config.client_secret.clone());

        let users = api.retrieve_users(&config.login_names).unwrap();
        let halt_streams: Vec<TwitchStream> = if config.halt_until_next_live {
            api.retrieve_streams(users.iter()).unwrap()
        } else {
            Vec::with_capacity(25)
        };

        StreamOrchestrator {
            api: api,
            config: config,
            filename: filename,
            modified: modified,
            halt_streams: halt_streams,
            streams: Vec::with_capacity(25),
            users: users,
        }
    }

    pub fn run(&mut self) {
        let (tx, rx): (Sender<StreamSubprocess>, Receiver<StreamSubprocess>) = mpsc::channel();
        let handle = thread::spawn(move || {
            for received in rx {
                println!("Processing item: {}", received.stream.title);
                let recording_full_path =
                    path::Path::new(&received.recording_path).join(&received.filename);
                let cleanup_full_path =
                    path::Path::new(&received.cleanup_path).join(&received.filename);

                let mut command = Command::new("ffmpeg");
                command.args(&[
                    "-nostdin",
                    "-y",
                    "-err_detect",
                    "ignore_err",
                    "-i",
                    recording_full_path.to_str().unwrap(),
                    "-c",
                    "copy",
                    cleanup_full_path.to_str().unwrap(),
                ]);

                command.spawn().unwrap().wait().unwrap();

                if let Err(_) = fs::remove_file(recording_full_path) {
                    continue;
                }

                let mut command = if cfg!(windows) {
                    let mut sub = Command::new("xcopy");
                    sub.args(&[
                        cleanup_full_path.to_str().unwrap(),
                        path::Path::new(&received.move_path).to_str().unwrap(),
                        "/j",
                    ]);
                    sub
                } else {
                    let mut sub = Command::new("cp");
                    sub.args(&[
                        cleanup_full_path.to_str().unwrap(),
                        path::Path::new(&received.move_path).to_str().unwrap(),
                    ]);
                    sub
                };

                command.spawn().unwrap().wait().unwrap();

                if let Err(_) = fs::remove_file(cleanup_full_path) {
                    continue;
                }
            }
        });

        let mut previous = 0;

        loop {
            let modified = fs::metadata(&self.filename).unwrap().modified().unwrap();
            if modified != self.modified {
                let local_config = Configuration::new(&self.filename).unwrap();
                if let Ok(users) = self.api.retrieve_users(&local_config.login_names) {
                    if local_config.halt_newly_added {
                        match self.api.retrieve_streams(users.iter()) {
                            Ok(streams) => {
                                let extension: Vec<TwitchStream> = streams
                                    .into_iter()
                                    .filter(|stream| {
                                        !self
                                            .streams
                                            .iter()
                                            .any(|process| &process.stream == stream)
                                    })
                                    .collect();
                                self.halt_streams.extend(extension);
                            }
                            Err(_) => {
                                eprintln!("Failed to retrieve stream information. Attempting to update on next iteration.");
                            }
                        }
                    }
                    self.config = local_config;
                    self.modified = modified;
                    self.users.clear();
                    self.users.extend(users);
                    println!("Successfully reloaded configuration.");
                } else {
                    eprintln!("Failed to retrieve user data. Attempting on next iteration.");
                }
            }

            match self.api.retrieve_streams(self.users.iter()) {
                Ok(streams) => {
                    self.halt_streams.retain(|stream| streams.contains(stream));
                    let startup_streams: Vec<TwitchStream> = streams
                        .into_iter()
                        .filter(|stream| !self.halt_streams.contains(stream))
                        .filter(|stream| {
                            !self.streams.iter().any(|process| &process.stream == stream)
                        })
                        .collect();
                    for stream in startup_streams.into_iter() {
                        let login_name = &self
                            .users
                            .iter()
                            .find(|user| &user.id == &stream.user_id)
                            .unwrap()
                            .login;
                        println!("Starting recording of {}'s stream.", login_name);

                        static UNSAFE_CHARACTERS: &'static [char] = &[
                            '\\', '/', ':', '*', '?', '"', '<', '>', '|', '\n', '\r', '\0',
                        ];
                        let now = time::SystemTime::now()
                            .duration_since(time::SystemTime::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();
                        let filename = format!(
                            "{} - {} - {} - {}.mp4",
                            login_name.trim(),
                            stream.id.trim(),
                            now,
                            stream.title.trim()
                        );
                        let filename =
                            filename.replace(|c: char| UNSAFE_CHARACTERS.contains(&c), "");
                        let subdirectory = login_name.trim();
                        let recording_path =
                            path::Path::new(&self.config.recording_path).join(&subdirectory);
                        let cleanup_path =
                            path::Path::new(&self.config.cleanup_path).join(&subdirectory);
                        let move_path = path::Path::new(&self.config.move_path).join(&subdirectory);
                        fs::create_dir_all(&recording_path).unwrap();
                        fs::create_dir_all(&cleanup_path).unwrap();
                        fs::create_dir_all(&move_path).unwrap();
                        let url = format!("https://www.twitch.tv/{}", login_name);

                        let mut command = Command::new("streamlink");
                        command
                            .args(&[
                                "--subprocess-errorlog",
                                "--twitch-disable-hosting",
                                url.as_str(),
                                "best",
                                "-o",
                                path::Path::new(&recording_path)
                                    .join(&filename)
                                    .to_str()
                                    .unwrap(),
                            ])
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null());

                        match command.spawn() {
                            Ok(child) => self.streams.push(StreamSubprocess {
                                stream,
                                child,
                                recording_path,
                                cleanup_path,
                                move_path,
                                filename,
                            }),
                            Err(e) => eprintln!("Application error: {}", e),
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Application error: {}", e);
                }
            }

            let mut updated: Vec<StreamSubprocess> = Vec::with_capacity(self.streams.len());
            for _ in 0..self.streams.len() {
                let mut stream_subprocess = self.streams.pop().unwrap();
                match stream_subprocess.child.try_wait() {
                    Ok(Some(Status)) => tx.send(stream_subprocess).unwrap(),
                    Ok(None) => updated.push(stream_subprocess),
                    _ => {}
                }
            }
            self.streams = updated;

            if self.streams.len() != previous {
                match self.streams.as_slice() {
                    [] => println!("Not recording."),
                    [single] => {
                        println!("Actively recording {}'s stream.", single.stream.user_name)
                    }
                    [first, second] => println!(
                        "Actively recording {}'s and {}'s stream.",
                        first.stream.user_name, second.stream.user_name
                    ),
                    [start @ .., last] => {
                        print!("Actively recording ");
                        for entry in start {
                            print!("{}, ", entry.stream.user_name);
                        }
                        println!("and {}'s streams.", last.stream.user_name);
                    }
                }

                previous = self.streams.len();
            }

            thread::sleep(time::Duration::from_secs(15));
        }

        handle.join().unwrap();
    }
}
