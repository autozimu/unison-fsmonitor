#[macro_use]
extern crate failure;
extern crate notify;
extern crate percent_encoding;

use failure::Error;
use notify::{DebouncedEvent, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::io::stdin;
use std::process::exit;
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::Duration;

type Result<R> = std::result::Result<R, Error>;

// TODO: handle sigint.

fn encode(s: &str) -> impl AsRef<str> {
    return percent_encoding::utf8_percent_encode(s, percent_encoding::SIMPLE_ENCODE_SET)
        .to_string();
}

fn decode<'a>(s: &'a str) -> impl AsRef<str> + 'a {
    return percent_encoding::percent_decode(s.as_bytes()).decode_utf8_lossy();
}

fn send_cmd(cmd: &str, args: &[&str]) {
    let mut output = cmd.to_owned();
    for arg in args {
        output += " ";
        output += &encode(arg).as_ref();
    }
    println!("{}", output);
}

fn ack() {
    send_cmd("OK", &[]);
}

fn changes(replicas: &[&str]) {
    send_cmd("CHANGES", replicas);
}

fn done() {
    send_cmd("DONE", &[]);
}

fn error(msg: &str) {
    send_cmd("ERROR", &[msg]);
    exit(1);
}

fn recv_cmd(rx: &Receiver<String>) -> Result<(String, Vec<String>)> {
    // TODO: Handle EOF
    let input = rx.try_recv()?;
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
    fspath: &str,
    rx: &Receiver<String>,
) -> Result<()> {
    watcher.watch(fspath, RecursiveMode::Recursive)?;
    ack();

    loop {
        let (cmd, _) = recv_cmd(rx)?;
        match cmd.as_str() {
            "ACK" => ack(),
            "LINK" => bail!("link following is not supported, please disable this option (-links)"),
            "DONE" => break,
            _ => error(&format!("Unexpected cmd: {}", cmd)),
        }
    }

    Ok(())
}

fn handle_fsevent<'a>(
    rx: &Receiver<DebouncedEvent>,
    replicas: impl Iterator<Item = &'a String>,
) -> Result<()> {
    if rx.recv_timeout(Duration::from_secs(1)).is_ok() {
        // TODO: notfiy matched replicas only.
        changes(
            replicas
                .map(String::as_str)
                .collect::<Vec<&str>>()
                .as_slice(),
        );
    }

    Ok(())
}

fn main() -> Result<()> {
    send_cmd("VERSION", &["1"]);

    let (stdin_tx, stdin_rx) = channel();
    thread::spawn(move || loop {
        let mut input = String::new();
        stdin().read_line(&mut input).unwrap();
        stdin_tx.send(input).unwrap();
    });

    let (cmd, args) = recv_cmd(&stdin_rx)?;
    if cmd != "VERSION" {
        bail!("Unexpected version cmd: {}", cmd);
    }
    let version = args.get(0);
    if version != Some(&"1".to_owned()) {
        bail!("Unexpected version: {:?}", version);
    }

    let mut replicas = HashSet::new();

    let delay = 1;
    let (fsevent_tx, fsevent_rx) = channel();
    let mut watcher: RecommendedWatcher = Watcher::new(fsevent_tx, Duration::from_secs(delay))?;

    loop {
        let (cmd, mut args) = recv_cmd(&stdin_rx)?;

        if cmd == "DEBUG" {
        } else if cmd == "START" {
            // Start observing replica.
            let replica = args.remove(0);
            let path = args.remove(0);
            add_to_watcher(&mut watcher, &path, &stdin_rx)?;
            replicas.insert(replica);
        } else if cmd == "WAIT" {
            // Start waiting for another replica.
        } else if cmd == "CHANGES" {
            // Get pending replicas.
            done();
        } else if cmd == "RESET" {
            // Stop observing replica.
            let replica = args.remove(0);
            watcher.unwatch(replica)?;
        } else {
            error(&format!("Unexpected root cmd: {}", cmd));
        }

        handle_fsevent(&fsevent_rx, replicas.iter())?;
    }
}
