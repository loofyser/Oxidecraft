//! The session's dirty set: which columns need meshing, and which job's
//! result is still current.
//!
//! Pure bookkeeping — no threads, no world, no assets. The session marks a
//! column dirty when an applied packet changes it, takes a job for a worker,
//! and tells the queue how the worker's result turned out. The rules:
//!
//! - a column appears at most once in the dirty set, however often it is
//!   marked;
//! - every change bumps the column's generation, and no generation is ever
//!   reused: a job's generation names the state of the column its snapshot
//!   was taken from, even across an unload that forgets the column and a
//!   packet that loads it anew;
//! - a completion whose generation is stale is discarded: the queue re-marks
//!   the column dirty and answers `false`. A fresh completion answers `true`
//!   and forgets the column;
//! - [`MeshQueue::mark_running`] keeps a handed-out column out of a second
//!   job until its completion has been seen;
//! - [`MeshQueue::mark_column_unloaded`] forgets the column and queues its
//!   four neighbours: a column that leaves the store changes the collar their
//!   meshes read.

use std::collections::{HashMap, VecDeque};

/// One column's build, as the worker receives it.
///
/// The generation is the column's state when the job was handed out; the
/// worker returns the job unchanged, and the queue compares the generation
/// against the column's current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshJob {
    /// The column's chunk x.
    pub cx: i32,
    /// The column's chunk z.
    pub cz: i32,
    /// The column's generation when the job was handed out.
    pub generation: u64,
}

/// A column's place in the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Waiting for a job; the generation a job handed out now would carry.
    Dirty(u64),
    /// Handed to a worker; the generation its job carried.
    Running(u64),
}

impl State {
    /// The generation the state carries.
    fn generation(self) -> u64 {
        match self {
            State::Dirty(generation) | State::Running(generation) => generation,
        }
    }
}

/// The session's dirty set.
#[derive(Debug)]
pub struct MeshQueue {
    /// The columns waiting for a job, in the order they were first marked.
    dirty: VecDeque<(i32, i32)>,
    /// Every outstanding column's state: dirty or running. A column leaves
    /// the map when its build completes fresh.
    states: HashMap<(i32, i32), State>,
    /// The last generation handed out. Every fresh mark and every re-queue of
    /// a forgotten column takes the next value, so no two jobs can ever carry
    /// the same generation for one column, not even across an unload.
    next_generation: u64,
}

impl MeshQueue {
    /// An empty queue.
    pub fn new() -> MeshQueue {
        MeshQueue {
            dirty: VecDeque::new(),
            states: HashMap::new(),
            next_generation: 0,
        }
    }

    /// Marks a column as needing a build, bumping its generation.
    ///
    /// A column already waiting keeps its one place in the dirty set; a column
    /// whose job is out stays out of it, and the completion of that job
    /// becomes stale.
    pub fn mark_dirty(&mut self, cx: i32, cz: i32) {
        let generation = self.fresh_generation();
        match self.states.get_mut(&(cx, cz)) {
            Some(State::Dirty(stored)) | Some(State::Running(stored)) => *stored = generation,
            None => {
                self.states.insert((cx, cz), State::Dirty(generation));
                self.dirty.push_back((cx, cz));
            }
        }
    }

    /// The next column to build, or `None` when none is waiting.
    ///
    /// The column stays in the dirty set until [`Self::mark_running`] records
    /// its job, so a caller that only reads the queue cannot lose the column.
    pub fn next_job(&mut self) -> Option<MeshJob> {
        let &(cx, cz) = self.dirty.front()?;
        match self.states.get(&(cx, cz)) {
            Some(State::Dirty(generation)) => Some(MeshJob {
                cx,
                cz,
                generation: *generation,
            }),
            // A running column is never at the front: `mark_running` takes it
            // out, and this arm only guards the invariant.
            Some(State::Running(_)) | None => None,
        }
    }

    /// Records a job as running: the column leaves the dirty set until its
    /// completion is seen.
    ///
    /// A job whose generation no longer matches the column's — the column
    /// changed between the job being handed out and this call — is not
    /// recorded: its completion is stale and will be discarded, and the
    /// column keeps its own place.
    pub fn mark_running(&mut self, job: MeshJob) {
        if self.states.get(&(job.cx, job.cz)) != Some(&State::Dirty(job.generation)) {
            return;
        }
        if let Some(position) = self
            .dirty
            .iter()
            .position(|&column| column == (job.cx, job.cz))
        {
            self.dirty.remove(position);
        }
        self.states
            .insert((job.cx, job.cz), State::Running(job.generation));
    }

    /// Records a finished build, answering whether its result is current.
    ///
    /// A current result — the column's generation still equals the job's — is
    /// taken: the column is forgotten and `true` answers. A stale result is
    /// discarded and `false` answers, and the column is queued again for a
    /// build that sees the change that made it stale. A column that vanished —
    /// an unload — is queued the same way: its empty build is what tells the
    /// window to drop the meshes it holds.
    pub fn complete(&mut self, job: MeshJob) -> bool {
        let state = self.states.get(&(job.cx, job.cz)).copied();
        if let Some(state) = state {
            if state.generation() == job.generation {
                self.forget(job.cx, job.cz);
                return true;
            }
            match state {
                // The column changed while the job was out: queue it again
                // with the generation the change stamped on it.
                State::Running(generation) => {
                    self.states
                        .insert((job.cx, job.cz), State::Dirty(generation));
                    self.dirty.push_back((job.cx, job.cz));
                }
                // It is already waiting: one place, the current generation.
                State::Dirty(_) => {}
            }
            return false;
        }
        // The column was forgotten: it was unloaded, or a new world replaced
        // it. The stale build is discarded and the column is queued once more
        // with a generation no earlier job carries, so a snapshot taken before
        // the column vanished can never answer as the new one's current build.
        let generation = self.fresh_generation();
        self.states
            .insert((job.cx, job.cz), State::Dirty(generation));
        self.dirty.push_back((job.cx, job.cz));
        false
    }

    /// Forgets an unloaded column and marks its four neighbours dirty.
    ///
    /// The column itself is not queued: the window drops its meshes from the
    /// unload report. Its neighbours are, because the collar their meshes read
    /// changed at the column's edge. An in-flight job for the column cannot be
    /// cancelled; its completion is stale and, when it arrives, queues the
    /// column once more so an empty build follows the meshes the window still
    /// holds.
    pub fn mark_column_unloaded(&mut self, cx: i32, cz: i32) {
        self.forget(cx, cz);
        for (nx, nz) in [(cx + 1, cz), (cx - 1, cz), (cx, cz + 1), (cx, cz - 1)] {
            self.mark_dirty(nx, nz);
        }
    }

    /// The number of distinct columns queued for a build: those waiting and
    /// those with a job out. Zero means nothing is outstanding.
    pub fn pending(&self) -> usize {
        self.states.len()
    }

    /// The current generation of every outstanding column, by coordinate.
    ///
    /// The tests read it to pin the bump rule; the session does not.
    pub fn generations(&self) -> HashMap<(i32, i32), u64> {
        self.states
            .iter()
            .map(|(&(cx, cz), state)| ((cx, cz), state.generation()))
            .collect()
    }

    /// Forgets a column entirely: its state and its place in the dirty set.
    fn forget(&mut self, cx: i32, cz: i32) {
        self.states.remove(&(cx, cz));
        if let Some(position) = self.dirty.iter().position(|&column| column == (cx, cz)) {
            self.dirty.remove(position);
        }
    }

    /// The next generation to hand out. Monotonic for the queue's lifetime, so
    /// a generation is never reused, whatever the column's history.
    fn fresh_generation(&mut self) -> u64 {
        self.next_generation += 1;
        self.next_generation
    }
}

impl Default for MeshQueue {
    fn default() -> Self {
        Self::new()
    }
}
