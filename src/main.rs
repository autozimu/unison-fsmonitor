use failure::{bail, Fallible};
use log::{debug, info};
use notify::{RawEvent, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::io::{stdin, BufRead};
use std::path::PathBuf;
use std::process::exit;
use std::sync::mpsc::channel;
use std::thread;

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

fn parse_input(input: &str) -> Fallible<(String, Vec<String>)> {
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

fn main() -> Fallible<()> {
    env_logger::init();

    // replica id => paths.
    let mut replicas: HashMap<_, HashSet<_>> = HashMap::new();

    // path => alias paths.
    let mut link_map: HashMap<_, HashSet<_>> = HashMap::new();

    // replica id => changed paths (relative).
    let mut pending_changes: HashMap<_, HashSet<PathBuf>> = HashMap::new();

    let (tx, rx) = channel();
    let tx_clone = tx.clone();
    thread::spawn(move || -> Fallible<()> {
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
    thread::spawn(move || -> Fallible<()> {
        loop {
            tx_clone.send(Event::FSEvent(fsevent_rx.recv()?))?;
        }
    });

    send_cmd("VERSION", &["1"]);

    let mut replica_path = PathBuf::new();

    loop {
        let event = rx.recv()?;

        match event {
            Event::Input(input) => {
                debug!("<< {}", input.trim());
                let (cmd, args) = parse_input(&input)?;

                match cmd.as_str() {
                    "VERSION" => {
                        let version = &args[0];
                        if version != "1" {
                            bail!("Unexpected version: {:?}", version);
                        }
                    }
                    "START" => {
                        // Start or append watching dirs.
                        // e.g.,
                        // START 123 root
                        // START 123 root subdir
                        let replica_id = args[0].clone();
                        replica_path = PathBuf::from(&args[1]);

                        if let Some(dir) = args.get(2) {
                            replica_path = replica_path.join(dir);
                        }
                        let mut is_watched = false;
                        for path in replicas.entry(replica_id.clone()).or_default().iter() {
                            if replica_path.starts_with(path) {
                                is_watched = true;
                            }
                        }
                        if !is_watched {
                            watcher.watch(&replica_path, RecursiveMode::Recursive)?;
                            replicas
                                .entry(replica_id)
                                .or_default()
                                .insert(replica_path.clone());
                        }

                        debug!("replicas: {:?}", replicas);
                        send_ack();
                    }
                    "DIR" => {
                        // Add sub-dir to watch list.
                        send_ack();
                    }
                    "LINK" => {
                        // Follow a link.
                        let path = replica_path.join(args.get(0).cloned().unwrap_or_default());
                        let realpath = path.canonicalize()?;

                        watcher.watch(&realpath, RecursiveMode::Recursive)?;
                        link_map.entry(realpath).or_default().insert(path);
                        debug!("link_map: {:?}", link_map);
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
                        for c in pending_changes.remove(replica).unwrap_or_default() {
                            send_recursive(&c.to_string_lossy());
                        }
                        debug!("pending_changes: {:?}", pending_changes);
                        send_done();
                    }
                    "RESET" => {
                        // Stop observing replica.
                        let replica_id = &args[0];
                        if let Some(paths) = replicas.remove(replica_id) {
                            // TODO: the same path might be watched for other replicas.
                            for path in paths {
                                watcher.unwatch(&path)?;
                            }
                        }
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

                if let Some(path) = fsevent.path {
                    let mut paths = vec![path.clone()];
                    // Get all possible symbolic links for this path.
                    for (realpath, links) in &link_map {
                        if let Ok(postfix) = path.strip_prefix(realpath) {
                            for link in links {
                                paths.push(link.join(postfix));
                            }
                        }
                    }

                    for path in paths {
                        for (id, replica_paths) in &replicas {
                            for replica_path in replica_paths {
                                if path.starts_with(replica_path) {
                                    matched_replica_ids.insert(id);
                                    pending_changes
                                        .entry(id.clone())
                                        .or_default()
                                        .insert(path.strip_prefix(replica_path)?.into());
                                }
                            }
                        }
                    }
                }

                if matched_replica_ids.is_empty() {
                    info!("No replica found for event.")
                }

                for id in &matched_replica_ids {
                    send_changes(id);
                }
            }
        }
    }
}
