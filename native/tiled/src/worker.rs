//! The [`Loader`] on a worker thread.
//!
//! The file-request protocol of [`crate::load`] with the parsing on a thread of its own: the
//! caller (Godot's main thread, in the native importer) reads the files the loader asks for and
//! hands them over; the worker parses them and answers each batch with one [`Event`]. The
//! caller never blocks: [`Worker::try_event`] returns at once.
//!
//! Everything that crosses the thread is a type of this crate or of `std` (paths, bytes, the
//! parsed [`Map`], [`Error`]s). This crate does not depend on Godot, so the worker cannot touch
//! an engine object: that is how the importer keeps Godot's types on the main thread without
//! godot-rust's `experimental-threads`.
//!
//! ```no_run
//! use tiled_core::worker::{Event, Worker};
//! use tiled_core::{Loader, Status};
//! let mut loader = Loader::new("maps/level.tmx", None);
//! let Status::NeedFiles = loader.step() else { unreachable!() };
//! let read = |paths: &[String]| -> Vec<(String, Option<Vec<u8>>)> {
//!     paths.iter().map(|p| (p.clone(), std::fs::read(p).ok())).collect()
//! };
//! let first = read(loader.needs());
//! let mut worker = Worker::spawn(loader, first).expect("thread");
//! let map = loop {
//!     // once per frame in a game
//!     match worker.try_event() {
//!         None => std::thread::yield_now(),
//!         Some(Event::Need { paths, .. }) => {
//!             worker.supply(read(&paths));
//!         }
//!         Some(Event::Done(map, _)) => break map,
//!         Some(Event::Failed(errors)) => panic!("{errors:?}"),
//!     }
//! };
//! println!("{} layers", map.all_layers().len());
//! ```
//!
//! [`Worker::spawn_preparing`] also prepares the map's tile layers on the worker once the map is
//! parsed ([`PreparedCells`]: the payloads decoded into blobs that wait only for the TileSet's
//! tile fields), so a build that follows does not decode them on the caller's thread.
//!
//! Dropping a [`Worker`] (or [`Worker::cancel`]) never waits: the thread stops at its next
//! message (it is parsing at most one batch), and its handle is kept until [`join_detached`]
//! (the importer calls it when the engine shuts down, so no thread outlives the library).

use crate::load::{Loader, Status};
use crate::prepare::PreparedCells;
use crate::{Error, ErrorKind, Map};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Mutex;
use std::thread::JoinHandle;

/// A file the caller read for the loader: its path (as the loader asked for it) and its bytes,
/// `None` when it does not exist or cannot be read.
pub type File = (String, Option<Vec<u8>>);

/// What the worker answers a batch of files with.
#[derive(Debug)]
pub enum Event {
    /// The loader needs more files: read `paths` and [`Worker::supply`] them. `images` are the
    /// image references the parse found since the last event
    /// ([`Loader::take_discovered_images`]), for loading textures in parallel.
    Need {
        /// The files to read.
        paths: Vec<String>,
        /// Images found since the last event (resolved paths).
        images: Vec<String>,
    },
    /// The map is loaded ([`Status::Done`]), with its tile layers prepared when the worker was
    /// started with [`Worker::spawn_preparing`].
    Done(Box<Map>, Option<PreparedCells>),
    /// The map could not be loaded ([`Status::Failed`]), or the thread stopped.
    Failed(Vec<Error>),
}

static RUNNING: AtomicUsize = AtomicUsize::new(0);
static DETACHED: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

/// The number of worker threads still running (detached ones included).
pub fn running() -> usize {
    RUNNING.load(Ordering::SeqCst)
}

/// Waits for every worker thread that was dropped or cancelled while it was still running. A
/// dropped worker stops at its next message, so this waits for at most one batch's parse each.
pub fn join_detached() {
    let handles: Vec<JoinHandle<()>> = match DETACHED.lock() {
        Ok(mut d) => d.drain(..).collect(),
        Err(_) => return,
    };
    for h in handles {
        let _ = h.join();
    }
}

fn detach(handle: JoinHandle<()>) {
    if let Ok(mut d) = DETACHED.lock() {
        // Forget the ones that have ended meanwhile.
        let (done, live): (Vec<_>, Vec<_>) = d.drain(..).partition(|h| h.is_finished());
        for h in done {
            let _ = h.join();
        }
        *d = live;
        if handle.is_finished() {
            let _ = handle.join();
        } else {
            d.push(handle);
        }
    }
}

/// Decrements the running count when the thread's function returns or unwinds.
struct RunningGuard;

impl Drop for RunningGuard {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A [`Loader`] running on its own thread.
#[derive(Debug)]
pub struct Worker {
    files: Option<Sender<Vec<File>>>,
    events: Receiver<Event>,
    handle: Option<JoinHandle<()>>,
    finished: bool,
}

impl Worker {
    /// Starts `loader` on a new thread with the files of its first batch: for a loader of the
    /// file system, step it once on the caller's side (it then asks for the map and the project
    /// file, which costs nothing) and pass those files; a zip loader ([`Loader::from_zip`])
    /// reads its archive itself, so pass it unstepped with no files and the whole parse runs on
    /// the worker. The worker supplies the batch, steps, and sends one [`Event`].
    pub fn spawn(loader: Loader, first: Vec<File>) -> std::io::Result<Worker> {
        Self::start(loader, first, false)
    }

    /// As [`spawn`](Self::spawn), and once the map is parsed the worker also prepares its tile
    /// layers ([`PreparedCells::prepare`], one more thread per layer), sent with
    /// [`Event::Done`].
    pub fn spawn_preparing(loader: Loader, first: Vec<File>) -> std::io::Result<Worker> {
        Self::start(loader, first, true)
    }

    fn start(loader: Loader, first: Vec<File>, prepare: bool) -> std::io::Result<Worker> {
        let (files_tx, files_rx) = channel::<Vec<File>>();
        let (events_tx, events_rx) = channel::<Event>();
        RUNNING.fetch_add(1, Ordering::SeqCst);
        let spawned = std::thread::Builder::new()
            .name("tiled-loader".into())
            .spawn(move || {
                let _guard = RunningGuard;
                run(loader, first, prepare, files_rx, events_tx);
            });
        let handle = match spawned {
            Ok(h) => h,
            Err(e) => {
                RUNNING.fetch_sub(1, Ordering::SeqCst);
                return Err(e);
            }
        };
        Ok(Worker {
            files: Some(files_tx),
            events: events_rx,
            handle: Some(handle),
            finished: false,
        })
    }

    /// Hands over the files of a [`Event::Need`]. False when the worker is gone.
    pub fn supply(&self, files: Vec<File>) -> bool {
        match &self.files {
            Some(tx) => tx.send(files).is_ok(),
            None => false,
        }
    }

    /// The worker's answer to the last batch, if it is there; never blocks.
    pub fn try_event(&mut self) -> Option<Event> {
        if self.finished {
            return None;
        }
        match self.events.try_recv() {
            Ok(e) => {
                if !matches!(e, Event::Need { .. }) {
                    self.finished = true;
                }
                Some(e)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.finished = true;
                Some(Event::Failed(vec![Error::new(
                    ErrorKind::Parse,
                    "ERROR: The map's loader thread stopped unexpectedly.",
                )]))
            }
        }
    }

    /// The worker's answer to the last batch, waiting for it (for tests and tools).
    pub fn wait_event(&mut self) -> Event {
        if self.finished {
            return Event::Failed(vec![Error::new(
                ErrorKind::Parse,
                "The loader has already finished.",
            )]);
        }
        match self.events.recv() {
            Ok(e) => {
                if !matches!(e, Event::Need { .. }) {
                    self.finished = true;
                }
                e
            }
            Err(_) => {
                self.finished = true;
                Event::Failed(vec![Error::new(
                    ErrorKind::Parse,
                    "ERROR: The map's loader thread stopped unexpectedly.",
                )])
            }
        }
    }

    /// Stops the worker without waiting (as dropping it does).
    pub fn cancel(self) {
        drop(self);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing the channel ends the thread at its next receive.
        self.files = None;
        if let Some(h) = self.handle.take() {
            detach(h);
        }
    }
}

/// The worker thread: supply a batch, step, answer; until the loader is done or the caller is
/// gone.
fn run(
    mut loader: Loader,
    first: Vec<File>,
    prepare: bool,
    files: Receiver<Vec<File>>,
    events: Sender<Event>,
) {
    let mut batch = first;
    loop {
        for (path, bytes) in batch {
            match bytes {
                Some(b) => loader.supply(&path, b),
                None => loader.missing(&path),
            }
        }
        let event = match loader.step() {
            Status::NeedFiles => Event::Need {
                paths: loader.needs().to_vec(),
                images: loader.take_discovered_images(),
            },
            Status::Done(map) => {
                let cells = prepare.then(|| PreparedCells::prepare(&map));
                Event::Done(map, cells)
            }
            Status::Failed(errors) => Event::Failed(errors),
        };
        let last = !matches!(event, Event::Need { .. });
        if events.send(event).is_err() || last {
            return;
        }
        batch = match files.recv() {
            Ok(b) => b,
            Err(_) => return,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixtures() -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures")
            .to_string_lossy()
            .into_owned()
    }

    fn read(paths: &[String]) -> Vec<File> {
        paths
            .iter()
            .map(|p| (p.clone(), std::fs::read(p).ok()))
            .collect()
    }

    fn load_threaded(path: &str) -> (Result<Map, Vec<Error>>, Vec<String>, usize) {
        let mut loader = Loader::new(path, None);
        assert!(matches!(loader.step(), Status::NeedFiles));
        let first = read(loader.needs());
        let mut w = Worker::spawn(loader, first).expect("spawn");
        let mut images = Vec::new();
        let mut rounds = 1;
        loop {
            match w.wait_event() {
                Event::Need { paths, images: i } => {
                    images.extend(i);
                    rounds += 1;
                    assert!(w.supply(read(&paths)));
                }
                Event::Done(m, cells) => {
                    assert!(cells.is_none(), "spawn does not prepare");
                    return (Ok(*m), images, rounds);
                }
                Event::Failed(e) => return (Err(e), images, rounds),
            }
        }
    }

    #[test]
    fn threaded_load_equals_synchronous_load() {
        for name in [
            "tilesets.tmx",
            "templates.tmx",
            "objects.tmx",
            "json_map.tmj",
        ] {
            let path = format!("{}/{name}", fixtures());
            let sync = crate::load::load_from_fs(&path, None).expect("sync load");
            let (threaded, images, _) = load_threaded(&path);
            let threaded = threaded.expect("threaded load");
            assert_eq!(sync.to_value(), threaded.to_value(), "{name}");
            assert_eq!(sync.images(), threaded.images(), "{name}");
            // Every image found early is one the finished map lists.
            for i in &images {
                assert!(threaded.images().contains(i), "{name}: {i}");
            }
        }
    }

    #[test]
    fn images_are_discovered_before_the_map_is_done() {
        let path = format!("{}/tilesets.tmx", fixtures());
        let (map, images, rounds) = load_threaded(&path);
        let map = map.expect("load");
        assert!(rounds >= 2, "external tilesets take a second round");
        assert!(!map.images().is_empty());
        assert!(!images.is_empty(), "no image reported before Done");
    }

    #[test]
    fn missing_map_fails() {
        let (r, _, _) = load_threaded("/nonexistent/map.tmx");
        let errors = r.expect_err("must fail");
        assert!(errors[0].message.contains("not found"), "{errors:?}");
    }

    #[test]
    fn zip_loads_entirely_on_the_worker() {
        let zip = std::fs::read(format!("{}/bundle.zip", fixtures())).expect("bundle.zip");
        let sync = Loader::from_zip(zip.clone(), "maps/zipmap.tmx", None)
            .run(|_| None)
            .expect("sync zip load");
        let mut w = Worker::spawn(Loader::from_zip(zip, "maps/zipmap.tmx", None), Vec::new())
            .expect("spawn");
        match w.wait_event() {
            Event::Done(m, _) => assert_eq!(m.to_value(), sync.to_value()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_preparing_worker_sends_the_prepared_tile_layers() {
        for name in ["layers.tmx", "infinite.tmx", "tilesets.tmx"] {
            let path = format!("{}/{name}", fixtures());
            let sync = crate::load::load_from_fs(&path, None).expect("sync load");
            let mut loader = Loader::new(&path, None);
            assert!(matches!(loader.step(), Status::NeedFiles));
            let first = read(loader.needs());
            let mut w = Worker::spawn_preparing(loader, first).expect("spawn");
            let (map, cells) = loop {
                match w.wait_event() {
                    Event::Need { paths, .. } => assert!(w.supply(read(&paths))),
                    Event::Done(m, cells) => break (m, cells),
                    Event::Failed(e) => panic!("{name}: {e:?}"),
                }
            };
            assert_eq!(map.to_value(), sync.to_value(), "{name}");
            let cells = cells.expect("prepared");
            assert_eq!(cells, PreparedCells::prepare(&sync), "{name}");
            assert!(cells.byte_size() > 0, "{name}");
        }
    }

    #[test]
    fn dropping_a_worker_mid_load_ends_its_thread() {
        let path = format!("{}/tilesets.tmx", fixtures());
        let mut loader = Loader::new(&path, None);
        assert!(matches!(loader.step(), Status::NeedFiles));
        let first = read(loader.needs());
        let w = Worker::spawn(loader, first).expect("spawn");
        // Dropped while it parses or waits for the tilesets.
        drop(w);
        join_detached();
        // Other tests run workers concurrently; this one's thread is joined, which is the
        // check (join_detached returned).
    }
}
