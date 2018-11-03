extern crate env_logger;
extern crate failure;
extern crate log;
extern crate notify;
extern crate percent_encoding;

use failure::{bail, err_msg, format_err, Error};
use log::{debug, warn};
use notify::{RawEvent, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};
use std::fs::canonicalize;
use std::io::{stdin, BufRead};
use std::path::{Path, PathBuf};
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

fn parse_input(input: &str) -> Result<(String, VecDeque<String>)> {
    let mut cmd = String::new();
    let mut args = VecDeque::new();
    for (idx, word) in input.split_whitespace().enumerate() {
        if idx == 0 {
            cmd = word.to_owned();
        } else {
            args.push_back(decode(word).as_ref().to_owned())
        }
    }
    Ok((cmd, args))
}

#[derive(Debug)]
enum Event {
    Input(String),
    FSEvent(RawEvent),
}

#[derive(Debug)]
struct Replica {
    root: PathBuf,
    dirs: HashSet<PathBuf>,
}

impl Replica {
    pub fn new<P: AsRef<Path>>(p: P) -> Replica {
        Replica {
            root: PathBuf::from(p.as_ref()),
            dirs: HashSet::new(),
        }
    }
}

fn main() -> Result<()> {
    env_logger::init();

    // real path => symbolic link paths.
    // let mut link_map: HashMap<_, HashSet<_>> = HashMap::new();

    // id => replica.
    let mut replicas: HashMap<_, Replica> = HashMap::new();
    debug!("replicas: {:?}", replicas);

    // replica id => changed paths.
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

    let mut replica_id = String::new();
    let mut replica_path = PathBuf::new();

    loop {
        let event = rx.recv()?;

        match event {
            Event::Input(input) => {
                debug!("<< {}", input.trim());
                let (cmd, mut args) = parse_input(&input)?;

                match cmd.as_str() {
                    "VERSION" => {
                        let version = &args[0];
                        if version != "1" {
                            bail!("Unexpected version: {:?}", version);
                        }
                    }
                    "START" => {
                        // Start observing replica.
                        replica_id = args[0].clone();
                        replica_path = PathBuf::from(&args[1]);
                        if let Some(dir) = args.get(2) {
                            replica_path = replica_path.join(dir);
                            // Clear previous observed paths.
                            replicas
                                .get_mut(&replica_id)
                                .ok_or_else(|| {
                                    format_err!("Replica with id {} not found!", replica_id)
                                })?.dirs
                                .retain(|path| {
                                    if path.starts_with(&replica_path) {
                                        let _ = watcher.unwatch(&path);
                                        false
                                    } else {
                                        true
                                    }
                                })
                        } else {
                            replicas.insert(replica_id.clone(), Replica::new(&replica_path));
                        }
                        debug!("replicas: {:?}", replicas);
                        send_ack();
                    }
                    "DIR" => {
                        // Adding dirs to watch.
                        let dir = args.pop_front().unwrap_or_default();
                        let fullpath = PathBuf::from(&replica_path).join(dir);

                        watcher.watch(&fullpath, RecursiveMode::NonRecursive)?;
                        replicas
                            .get_mut(&replica_id)
                            .ok_or_else(|| {
                                format_err!("Replica with id {} not found!", replica_id)
                            })?.dirs
                            .insert(fullpath);
                        debug!("replicas: {:?}", replicas);
                        send_ack();
                    }
                    "LINK" => {
                        // Follow a link.
                        // let path = args.remove(0);
                        // let fullpath = PathBuf::from(&replica_path).join(path);
                        // let realpath = canonicalize(&fullpath)?;

                        // watcher.watch(&realpath, RecursiveMode::Recursive)?;
                        // link_map.entry(realpath).or_default().insert(fullpath);
                        send_ack();
                    }
                    "WAIT" => {
                        // Start waiting replica.
                        let replica_id = &args[0];
                        if !replicas.contains_key(replica_id) {
                            send_error(&format!("Unknown replica: {}", replica_id));
                        }
                    }
                    "CHANGES" => {
                        // Request pending changes.
                        let replica = &args[0];
                        let replica_changes = pending_changes.remove(replica).unwrap_or_default();
                        for c in replica_changes {
                            send_recursive(&c.to_string_lossy());
                        }
                        debug!("pending_changes: {:?}", pending_changes);
                        send_done();
                    }
                    "RESET" => {
                        // Stop observing replica.
                        let replica_id = &args[0];
                        for path in &replicas
                            .get(replica_id)
                            .ok_or_else(|| {
                                format_err!("Replica with id {} not found!", replica_id)
                            })?.dirs
                        {
                            // TODO: the same path might be watched for other replicas.
                            watcher.unwatch(&path)?;
                        }
                        replicas.remove(replica_id);
                        debug!("replicas: {:?}", replicas);
                    }
                    "DEBUG" | "DONE" => {
                        // TODO: update debug level.
                    }
                    _ => {
                        send_error(&format!("Unexpected cmd: {}", cmd));
                    }
                }
            }
            Event::FSEvent(fsevent) => {
                debug!("FS event: {:?}", fsevent);

                let mut matched_replica_ids = HashSet::new();

                if let Some(file_path) = fsevent.path {
                    for (id, replica) in &replicas {
                        if replica.dirs.contains(&file_path) || file_path
                            .parent()
                            .map(|p| replica.dirs.contains(p))
                            .unwrap_or_default()
                        {
                            matched_replica_ids.insert(id);
                            let relative_path = file_path.strip_prefix(&replica.root)?;
                            pending_changes
                                .entry(id.clone())
                                .or_default()
                                .insert(relative_path.into());
                        }
                    }
                }

                if matched_replica_ids.is_empty() {
                    warn!("No replica found for event!")
                }

                for id in &matched_replica_ids {
                    send_changes(id);
                }
            }
        }
    }
}
