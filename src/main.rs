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
    fn watch(&mut self, _path: &Path, _recursive_mode: RecursiveMode) -> Fallible<()> {
        Ok(())
    }

    fn unwatch(&mut self, _path: &Path) -> Fallible<()> {
        Ok(())
    }
}

impl Watch for RecommendedWatcher {
    fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Fallible<()> {
        Ok(notify::Watcher::watch(self, path, recursive_mode)?)
    }

    fn unwatch(&mut self, path: &Path) -> Fallible<()> {
        Ok(notify::Watcher::unwatch(self, path)?)
    }
}

type Id = String;

#[derive(Debug)]
struct Replica {
    pub root: PathBuf,
    /// Currently being watched paths.
    pub paths: HashSet<PathBuf>,
    /// Paths of pending changes. Paths are relative as required by unison.
    pub pending_changes: HashSet<PathBuf>,
}

impl Replica {
    pub fn new(root: PathBuf) -> Replica {
        Replica {
            root,
            paths: HashSet::new(),
            pending_changes: HashSet::new(),
        }
    }

    /// Check if path is being watched in this replica.
    pub fn is_watching(&self, path: &Path) -> bool {
        self.paths.iter().any(|base| path.starts_with(base))
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
        Self {
            current_path: PathBuf::new(),
            replicas: HashMap::new(),
            link_map: HashMap::new(),
            watcher,
            writer,
        }
    }

    pub fn is_watching(&self, path: &Path) -> bool {
        self.replicas
            .values()
            .any(|replica| replica.is_watching(path))
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

                        self.send_cmd("VERSION", &["1"]);
                    }
                    "START" => {
                        // Start or append watching dirs.
                        // e.g.,
                        // START 123 root
                        // START 123 root subdir
                        let replica_id = args[0].clone();
                        let root = PathBuf::from(&args[1]);
                        self.current_path = root.clone();

                        if let Some(dir) = args.get(2) {
                            self.current_path = self.current_path.join(dir);
                        }

                        let replica = self
                            .replicas
                            .entry(replica_id)
                            .or_insert_with(|| Replica::new(root));

                        if !replica.is_watching(&self.current_path) {
                            self.watcher
                                .watch(&self.current_path, RecursiveMode::Recursive)?;
                            replica.paths.insert(self.current_path.clone());
                        }

                        debug!("replicas: {:?}", self.replicas);
                        self.send_ack();
                    }
                    "DIR" => {
                        // Add sub-dir to watch list.
                        self.send_ack();
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
                        self.send_ack();
                    }
                    "WAIT" => {
                        // Start waiting replica.
                        let replica_id = &args[0];
                        if !self.replicas.contains_key(replica_id) {
                            self.send_error(&format!("Unknown replica: {}", replica_id));
                        }
                    }
                    "CHANGES" => {
                        // Request pending changes.
                        let replica_id = &args[0];
                        let mut changed_paths = HashSet::new();
                        if let Some(replica) = self.replicas.get_mut(replica_id) {
                            changed_paths.extend(replica.pending_changes.drain());
                        }
                        for p in changed_paths {
                            self.send_recursive(&p);
                        }
                        self.send_done();
                    }
                    "RESET" => {
                        // Stop observing replica.
                        let replica_id = &args[0];
                        if let Some(replica) = self.replicas.remove(replica_id) {
                            for path in &replica.paths {
                                if !self.is_watching(&path) {
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
                        self.send_error(&format!("Unrecognized cmd: {}", cmd));
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
                            if let Ok(relative_path) = path.strip_prefix(&replica.root) {
                                matched_replica_ids.insert(id.clone());
                                // Unison requires relative path for changes.
                                replica.pending_changes.insert(relative_path.into());
                            }
                        }
                    }
                }

                if matched_replica_ids.is_empty() {
                    info!("No replica found for event.")
                }

                for id in &matched_replica_ids {
                    self.send_changes(id);
                }
            }
        }

        Ok(())
    }

    fn send_cmd(&mut self, cmd: &str, args: &[&str]) {
        let mut output = cmd.to_owned();
        for arg in args {
            output += " ";
            output += &encode(arg).as_ref();
        }

        debug!(">> {}", output);
        let _ = writeln!(self.writer, "{}", output);
    }

    fn send_ack(&mut self) {
        self.send_cmd("OK", &[]);
    }

    fn send_changes(&mut self, replica: &str) {
        self.send_cmd("CHANGES", &[replica]);
    }

    fn send_recursive(&mut self, path: &Path) {
        self.send_cmd("RECURSIVE", &[&path.to_string_lossy()]);
    }

    fn send_done(&mut self) {
        self.send_cmd("DONE", &[]);
    }

    fn send_error(&mut self, msg: &str) {
        self.send_cmd("ERROR", &[msg]);
        exit(1);
    }
}

#[cfg(test)]
mod test {
    use crate::*;
    use notify::Op;
    use std::io::Cursor;

    struct Watcher {}

    impl Watch for Watcher {}

    #[test]
    fn test_version() {
        let mut monitor = Monitor::new(Watcher {}, Cursor::new(vec![]));

        monitor
            .handle_event(Event::Input("VERSION 1\n".into()))
            .unwrap();

        monitor.writer.set_position(0);
        assert_eq!(
            monitor
                .writer
                .lines()
                .collect::<Result<Vec<String>, _>>()
                .unwrap(),
            vec!["VERSION 1"]
        );
    }

    #[test]
    fn test_start() {
        let mut monitor = Monitor::new(Watcher {}, Cursor::new(vec![]));
        let id = "123";
        let root = PathBuf::from("/tmp/sample");

        monitor
            .handle_event(Event::Input(format!(
                "START {} {}\n",
                id,
                root.to_string_lossy()
            )))
            .unwrap();

        assert_eq!(monitor.replicas.len(), 1);
        assert!(monitor.replicas.contains_key(id));
        assert_eq!(monitor.replicas.get(id).unwrap().root, root);
        assert!(monitor.replicas.get(id).unwrap().paths.contains(&root));
        monitor.writer.set_position(0);
        assert_eq!(
            monitor
                .writer
                .lines()
                .collect::<Result<Vec<String>, _>>()
                .unwrap(),
            vec!["OK"]
        );
    }

    #[test]
    fn test_start_with_subdir() {
        let mut monitor = Monitor::new(Watcher {}, Cursor::new(vec![]));
        let id = "123";
        let root = PathBuf::from("/tmp/sample");
        let subdir = PathBuf::from("subdir");

        monitor
            .handle_event(Event::Input(format!(
                "START {} {} {}\n",
                id,
                root.to_string_lossy(),
                subdir.to_string_lossy()
            )))
            .unwrap();

        assert_eq!(monitor.replicas.len(), 1);
        assert!(monitor.replicas.contains_key(id));
        assert_eq!(monitor.replicas.get(id).unwrap().root, root);
        assert!(monitor
            .replicas
            .get(id)
            .unwrap()
            .paths
            .contains(&root.join(&subdir)));
        monitor.writer.set_position(0);
        assert_eq!(
            monitor
                .writer
                .lines()
                .collect::<Result<Vec<String>, _>>()
                .unwrap(),
            vec!["OK"]
        );
    }

    #[test]
    fn test_dir() {
        let mut monitor = Monitor::new(Watcher {}, Cursor::new(vec![]));

        monitor.handle_event(Event::Input("DIR\n".into())).unwrap();

        monitor.writer.set_position(0);
        assert_eq!(
            monitor
                .writer
                .lines()
                .collect::<Result<Vec<String>, _>>()
                .unwrap(),
            vec!["OK"]
        );
    }

    #[test]
    fn test_dir_with_dir() {
        let mut monitor = Monitor::new(Watcher {}, Cursor::new(vec![]));

        monitor
            .handle_event(Event::Input("DIR dir\n".into()))
            .unwrap();

        monitor.writer.set_position(0);
        assert_eq!(
            monitor
                .writer
                .lines()
                .collect::<Result<Vec<String>, _>>()
                .unwrap(),
            vec!["OK"]
        );
    }

    #[test]
    fn test_changes() {
        let mut monitor = Monitor::new(Watcher {}, Cursor::new(vec![]));
        let id = "123";
        let root = "/tmp/sample";
        let filename = "filename";

        monitor
            .handle_event(Event::Input(format!("START {} {}\n", id, root)))
            .unwrap();
        monitor
            .handle_event(Event::FSEvent(RawEvent {
                path: Option::Some(PathBuf::from(root).join(filename)),
                op: Result::Ok(Op::CREATE),
                cookie: None,
            }))
            .unwrap();
        monitor
            .handle_event(Event::Input(format!("CHANGES {}\n", id)))
            .unwrap();

        monitor.writer.set_position(0);
        assert_eq!(
            monitor
                .writer
                .lines()
                .collect::<Result<Vec<String>, _>>()
                .unwrap(),
            vec![
                "OK",
                &format!("CHANGES {}", id),
                &format!("RECURSIVE {}", filename),
                "DONE"
            ]
        );
    }

    #[test]
    fn test_changes_with_subdir() {
        let mut monitor = Monitor::new(Watcher {}, Cursor::new(vec![]));
        let id = "123";
        let root = "/tmp/sample";
        let subdir = "subdir";
        let filename = "filename";

        monitor
            .handle_event(Event::Input(format!("START {} {} {}\n", id, root, subdir)))
            .unwrap();
        monitor
            .handle_event(Event::FSEvent(RawEvent {
                path: Option::Some(PathBuf::from(root).join(subdir).join(filename)),
                op: Result::Ok(Op::CREATE),
                cookie: None,
            }))
            .unwrap();
        monitor
            .handle_event(Event::Input(format!("CHANGES {}\n", id)))
            .unwrap();

        monitor.writer.set_position(0);
        assert_eq!(
            monitor
                .writer
                .lines()
                .collect::<Result<Vec<String>, _>>()
                .unwrap(),
            vec![
                "OK",
                &format!("CHANGES {}", id),
                &format!(
                    "RECURSIVE {}",
                    PathBuf::from(subdir).join(filename).to_string_lossy()
                ),
                "DONE"
            ]
        );
    }
}

fn main() -> Fallible<()> {
    env_logger::init();

    let (fsevent_tx, fsevent_rx) = channel();
    let watcher: RecommendedWatcher = notify::Watcher::new_raw(fsevent_tx)?;

    let stdout = stdout();
    let stdout = stdout.lock();
    let mut monitor = Monitor::new(watcher, stdout);

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

    thread::spawn(move || -> Fallible<()> {
        for event in fsevent_rx {
            tx.send(Event::FSEvent(event))?;
        }
        Ok(())
    });

    for event in rx {
        monitor.handle_event(event)?;
    }

    Ok(())
}
