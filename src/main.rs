use failure::{bail, Fallible};
use log::{debug, info};
use notify::{RawEvent, RecommendedWatcher, RecursiveMode};
use std::collections::{HashMap, HashSet};
use std::io::{stdin, stdout, BufRead, Write};
use std::path::{Path, PathBuf};
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

trait Watch {
    fn watch<P: AsRef<Path>>(&mut self, _path: P, _recursive_mode: RecursiveMode) -> Fallible<()> {
        Ok(())
    }

    fn unwatch<P: AsRef<Path>>(&mut self, _path: P) -> Fallible<()> {
        Ok(())
    }
}

impl Watch for RecommendedWatcher {
    fn watch<P: AsRef<Path>>(&mut self, path: P, recursive_mode: RecursiveMode) -> Fallible<()> {
        Ok(notify::Watcher::watch(self, path, recursive_mode)?)
    }

    fn unwatch<P: AsRef<Path>>(&mut self, path: P) -> Fallible<()> {
        Ok(notify::Watcher::unwatch(self, path)?)
    }
}

type Id = String;

#[derive(Debug, Default)]
struct Replica {
    pub paths: HashSet<PathBuf>,
    pub pending_changes: HashSet<PathBuf>,
}

impl Replica {
    /// Check if path is contained in this replica.
    pub fn contains_path(&self, path: &Path) -> bool {
        for base in &self.paths {
            if path.starts_with(base) {
                return true;
            }
        }
        false
    }
}

struct Monitor<WATCH: Watch, WRITE: Write> {
    pub current_path: PathBuf,
    pub replicas: HashMap<Id, Replica>,
    pub link_map: HashMap<PathBuf, HashSet<PathBuf>>,
    pub watcher: WATCH,
    pub writer: WRITE,
}

impl<WATCH: Watch, WRITE: Write> Monitor<WATCH, WRITE> {
    pub fn new(watcher: WATCH, writer: WRITE) -> Self {
        send_cmd("VERSION", &["1"]);

        Self {
            current_path: PathBuf::new(),
            replicas: HashMap::new(),
            link_map: HashMap::new(),
            watcher,
            writer,
        }
    }

    pub fn contains_path(&self, path: &Path) -> bool {
        self.replicas
            .values()
            .any(|replica| replica.contains_path(path))
    }

    pub fn handle_event(&mut self, event: Event) -> Fallible<()> {
        debug!("event: {:?}", event);

        match event {
            Event::Input(input) => {
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
                        self.current_path = PathBuf::from(&args[1]);

                        if let Some(dir) = args.get(2) {
                            self.current_path = self.current_path.join(dir);
                        }

                        if !self.contains_path(&self.current_path) {
                            self.watcher
                                .watch(&self.current_path, RecursiveMode::Recursive)?;
                            self.replicas
                                .entry(replica_id)
                                .or_default()
                                .paths
                                .insert(self.current_path.clone());
                        }

                        debug!("replicas: {:?}", self.replicas);
                        send_ack();
                    }
                    "DIR" => {
                        // Add sub-dir to watch list.
                        send_ack();
                    }
                    "LINK" => {
                        // Follow a link.
                        let path = self
                            .current_path
                            .join(args.get(0).cloned().unwrap_or_default());
                        let realpath = path.canonicalize()?;

                        self.watcher.watch(&realpath, RecursiveMode::Recursive)?;
                        self.link_map.entry(realpath).or_default().insert(path);
                        debug!("link_map: {:?}", self.link_map);
                        send_ack();
                    }
                    "WAIT" => {
                        // Start waiting replica.
                        let replica_id = &args[0];
                        if !self.replicas.contains_key(replica_id) {
                            send_error(&format!("Unknown replica: {}", replica_id));
                        }
                    }
                    "CHANGES" => {
                        // Request pending changes.
                        let replica_id = &args[0];
                        if let Some(replica) = self.replicas.get_mut(replica_id) {
                            for c in replica.pending_changes.drain() {
                                send_recursive(&c.to_string_lossy());
                            }
                        }
                        send_done();
                    }
                    "RESET" => {
                        // Stop observing replica.
                        let replica_id = &args[0];
                        if let Some(replica) = self.replicas.remove(replica_id) {
                            for path in replica.paths {
                                if !self.contains_path(&path) {
                                    self.watcher.unwatch(&path)?;
                                }
                            }
                        }
                        debug!("replicas: {:?}", self.replicas);
                    }
                    "DEBUG" | "DONE" => {
                        // TODO: update debug level.
                    }
                    _ => {
                        send_error(&format!("Unrecognized cmd: {}", cmd));
                    }
                }
            }
            Event::FSEvent(fsevent) => {
                let mut matched_replica_ids = HashSet::new();

                if let Some(path) = fsevent.path {
                    let mut paths = vec![path.clone()];
                    // Get all possible symbolic links for this path.
                    for (realpath, links) in &self.link_map {
                        if let Ok(postfix) = path.strip_prefix(realpath) {
                            for link in links {
                                paths.push(link.join(postfix));
                            }
                        }
                    }

                    for (id, replica) in self.replicas.iter_mut() {
                        for path in &paths {
                            for dir in &replica.paths {
                                if let Ok(relative_path) = path.strip_prefix(dir) {
                                    matched_replica_ids.insert(id);
                                    replica.pending_changes.insert(relative_path.into());
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

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::*;

    struct Watcher {}

    impl Watch for Watcher {}

    #[test]
    fn test_version() {
        let mut monitor = Monitor::new(Watcher {}, stdout());

        monitor
            .handle_event(Event::Input("VERSION 1".into()))
            .unwrap();
    }

    #[test]
    fn test_watch_path() {
        let mut monitor = Monitor::new(Watcher {}, stdout());

        monitor
            .handle_event(Event::Input("START 123 /tmp/sample".into()))
            .unwrap();

        assert!(monitor.contains_path(&PathBuf::from("/tmp/sample")));
    }

    #[test]
    fn test_watch_path_with_subpath() {
        let mut monitor = Monitor::new(Watcher {}, stdout());

        monitor
            .handle_event(Event::Input("START 123 /tmp/sample subdir".into()))
            .unwrap();

        assert!(monitor.contains_path(&PathBuf::from("/tmp/sample/subdir")));
    }
}

fn main() -> Fallible<()> {
    env_logger::init();

    let (fsevent_tx, fsevent_rx) = channel();
    let watcher: RecommendedWatcher = notify::Watcher::new_raw(fsevent_tx)?;

    let mut monitor = Monitor::new(watcher, stdout());

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

    let tx_clone = tx.clone();
    thread::spawn(move || -> Fallible<()> {
        for event in fsevent_rx {
            tx_clone.send(Event::FSEvent(event))?;
        }
        Ok(())
    });

    for event in rx {
        monitor.handle_event(event)?;
    }

    Ok(())
}
