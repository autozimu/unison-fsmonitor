#[macro_use]
extern crate failure;
extern crate notify;
extern crate percent_encoding;
#[macro_use]
extern crate log;
extern crate env_logger;

use failure::Error;
use notify::{RawEvent, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::io::{stdin, BufRead};
use std::path::PathBuf;
use std::process::exit;
use std::sync::mpsc::channel;
use std::thread;

type Result<R> = std::result::Result<R, Error>;

fn encode(s: &str) -> impl AsRef<str> {
    percent_encoding::utf8_percent_encode(s, percent_encoding::SIMPLE_ENCODE_SET).to_string()
}

fn decode<'a>(s: &'a str) -> impl AsRef<str> + 'a {
    percent_encoding::percent_decode(s.as_bytes()).decode_utf8_lossy()
}

fn send_cmd(cmd: &str, args: &[&str]) {
    let mut output = cmd.to_owned();
    for arg in args {
        output += " ";
        output += &encode(arg).as_ref();
    }

    debug!(">> {}", output);
    println!("{}", output);
}

fn send_ack() {
    send_cmd("OK", &[]);
}

fn send_changes(replica: &str) {
    send_cmd("CHANGES", &[replica]);
}

fn send_recursive(path: &str) {
    send_cmd("RECURSIVE", &[path]);
}

fn send_done() {
    send_cmd("DONE", &[]);
}

fn send_error(msg: &str) {
    send_cmd("ERROR", &[msg]);
    exit(1);
}

fn parse_input(input: &str) -> Result<(String, Vec<String>)> {
    let mut cmd = String::new();
    let mut args = vec![];
    for (idx, word) in input.split_whitespace().enumerate() {
        if idx == 0 {
            cmd = word.to_owned();
        } else {
            args.push(decode(word).as_ref().to_owned())
        }
    }
    Ok((cmd, args))
}

#[derive(Debug)]
enum Event {
    Input(String),
    FSEvent(RawEvent),
}

fn main() -> Result<()> {
    env_logger::init();

    // id => path.
    let mut replicas = HashMap::new();
    debug!("replicas: {:?}", replicas);

    // id => changed paths.
    let mut pending_changes: HashMap<String, HashSet<PathBuf>> = HashMap::new();
    debug!("pending_changes: {:?}", pending_changes);

    let (tx, rx) = channel();
    let tx_clone = tx.clone();
    thread::spawn(move || -> Result<()> {
        let stdin = stdin();
        let mut handle = stdin.lock();

        loop {
            let mut input = String::new();
            handle.read_line(&mut input)?;
            tx_clone.send(Event::Input(input))?;
        }
    });

    let (fsevent_tx, fsevent_rx) = channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(fsevent_tx)?;
    let tx_clone = tx.clone();
    thread::spawn(move || -> Result<()> {
        loop {
            tx_clone.send(Event::FSEvent(fsevent_rx.recv()?))?;
        }
    });

    send_cmd("VERSION", &["1"]);

    loop {
        let event = rx.recv()?;

        match event {
            Event::Input(input) => {
                debug!("<< {}", input.trim());
                let (cmd, mut args) = parse_input(&input)?;

                if cmd == "VERSION" {
                    let version = args.remove(0);
                    if version != "1" {
                        bail!("Unexpected version: {:?}", version);
                    }
                } else if cmd == "DEBUG" {
                } else if cmd == "START" {
                    // Start observing replica.
                    let replica = args.remove(0);
                    let path = args.remove(0);

                    watcher.watch(&path, RecursiveMode::Recursive)?;
                    replicas.insert(replica, path);
                    debug!("replicas: {:?}", replicas);
                    send_ack();
                } else if cmd == "DIR" {
                    send_ack();
                } else if cmd == "LINK" {
                    bail!("link following is not supported, please disable this option (-links)");
                } else if cmd == "DONE" {
                } else if cmd == "WAIT" {
                    // Start waiting replica.
                    let replica = args.remove(0);
                    if !replicas.contains_key(&replica) {
                        send_error(&format!("Unknown replica: {}", replica));
                    }
                } else if cmd == "CHANGES" {
                    // Request pending changes.
                    let replica = args.remove(0);
                    let replica_changes = pending_changes.remove(&replica).unwrap_or_default();
                    for c in replica_changes {
                        send_recursive(c.to_string_lossy().as_ref());
                    }
                    debug!("pending_changes: {:?}", pending_changes);
                    send_done();
                } else if cmd == "RESET" {
                    // Stop observing replica.
                    let replica = args.remove(0);
                    watcher.unwatch(&replica)?;
                    replicas.remove(&replica);
                    debug!("replicas: {:?}", replicas);
                } else {
                    send_error(&format!("Unexpected cmd: {}", cmd));
                }
            }
            Event::FSEvent(fsevent) => {
                debug!("FS event: {:?}", fsevent);

                let mut matched_replicas = HashSet::new();

                if let Some(file_path) = fsevent.path {
                    for (replica, replica_path) in &replicas {
                        if file_path.starts_with(replica_path) {
                            matched_replicas.insert(replica.clone());
                            let relative_path = file_path.strip_prefix(replica_path)?;
                            pending_changes
                                .entry(replica.clone())
                                .or_default()
                                .insert(relative_path.into());
                            debug!("pending_changes: {:?}", pending_changes);
                        }
                    }
                }

                for replica in matched_replicas {
                    send_changes(&replica);
                }
            }
        }
    }
}
