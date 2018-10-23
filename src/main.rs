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
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

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

fn ack() {
    send_cmd("OK", &[]);
}

fn changes(replica: &str) {
    send_cmd("CHANGES", &[replica]);
}

fn recursive(path: &str) {
    send_cmd("RECURSIVE", &[path]);
}

fn done() {
    send_cmd("DONE", &[]);
}

fn error(msg: &str) {
    send_cmd("ERROR", &[msg]);
    exit(1);
}

fn parse_input(input: &str) -> Result<(String, Vec<String>)> {
    debug!("<< {}", input.trim());

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

fn add_to_watcher(
    watcher: &mut RecommendedWatcher,
    path: &str,
    rx: &Receiver<String>,
) -> Result<()> {
    watcher.watch(path, RecursiveMode::Recursive)?;
    ack();

    loop {
        let input = rx.recv()?;
        let (cmd, _) = parse_input(&input)?;
        match cmd.as_str() {
            "DIR" => ack(),
            "LINK" => bail!("link following is not supported, please disable this option (-links)"),
            "DONE" => break,
            _ => error(&format!("Unexpected cmd: {}", cmd)),
        }
    }

    Ok(())
}

fn handle_fsevent(
    rx: &Receiver<RawEvent>,
    replicas: &HashMap<String, String>,
    pending_changes: &mut HashMap<String, HashSet<PathBuf>>,
) -> Result<()> {
    for event in rx.try_iter() {
        debug!("FS event: {:?}", event);

        if let Some(file_path) = event.path {
            for (replica, replica_path) in replicas {
                if file_path.starts_with(replica_path) {
                    let relative_path = file_path.strip_prefix(replica_path)?;
                    pending_changes
                        .entry(replica.clone())
                        .or_default()
                        .insert(relative_path.into());
                    debug!("pending_changes: {:?}", pending_changes);
                }
            }
        }
    }

    for replica in pending_changes.keys() {
        changes(replica);
    }

    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();

    send_cmd("VERSION", &["1"]);

    let (stdin_tx, stdin_rx) = channel();
    thread::spawn(move || {
        let stdin = stdin();
        let mut handle = stdin.lock();

        loop {
            let mut input = String::new();
            if let Err(err) = handle.read_line(&mut input) {
                debug!("Failed to read input: {:?}", err);
                break;
            }
            if let Err(err) = stdin_tx.send(input) {
                debug!("Failed to send input: {:?}", err);
                break;
            }
        }
    });

    let input = stdin_rx.recv()?;
    let (cmd, args) = parse_input(&input)?;
    if cmd != "VERSION" {
        bail!("Unexpected version cmd: {}", cmd);
    }
    let version = args.get(0);
    if version != Some(&"1".to_owned()) {
        bail!("Unexpected version: {:?}", version);
    }

    // id => path.
    let mut replicas = HashMap::new();
    debug!("replicas: {:?}", replicas);

    // id => changed paths.
    let mut pending_changes = HashMap::new();
    debug!("pending_changes: {:?}", pending_changes);

    let (fsevent_tx, fsevent_rx) = channel();
    let mut watcher: RecommendedWatcher = Watcher::new_raw(fsevent_tx)?;

    loop {
        handle_fsevent(&fsevent_rx, &replicas, &mut pending_changes)?;

        let input = match stdin_rx.recv_timeout(Duration::from_secs(1)) {
            Ok(input) => {
                if input.is_empty() {
                    break;
                }
                input
            }
            Err(RecvTimeoutError::Timeout) => {
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                break;
            }
        };

        let (cmd, mut args) = parse_input(&input)?;

        if cmd == "DEBUG" {
        } else if cmd == "START" {
            // Start observing replica.
            let replica = args.remove(0);
            let path = args.remove(0);
            add_to_watcher(&mut watcher, &path, &stdin_rx)?;
            replicas.insert(replica, path);
            debug!("replicas: {:?}", replicas);
        } else if cmd == "WAIT" {
            // Start waiting replica.
            let replica = args.remove(0);
            if !replicas.contains_key(&replica) {
                error(&format!("Unknown replica: {}", replica));
            }
        } else if cmd == "CHANGES" {
            // Request pending changes.
            let replica = args.remove(0);
            let replica_changes = pending_changes.remove(&replica).unwrap_or_default();
            for c in replica_changes {
                recursive(c.to_string_lossy().as_ref());
            }
            debug!("pending_changes: {:?}", pending_changes);
            done();
        } else if cmd == "RESET" {
            // Stop observing replica.
            let replica = args.remove(0);
            watcher.unwatch(&replica)?;
            replicas.remove(&replica);
            debug!("replicas: {:?}", replicas);
        } else {
            error(&format!("Unexpected cmd: {}", cmd));
        }
    }

    Ok(())
}
