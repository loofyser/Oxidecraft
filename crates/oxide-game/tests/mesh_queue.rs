//! The mesh queue's bookkeeping, with no threads and no world: which columns
//! are dirty, which job is current, and what a completion does to both.

use oxide_game::mesh_queue::{MeshJob, MeshQueue};

/// Takes the next job, records it as running, and returns it.
fn take(queue: &mut MeshQueue) -> MeshJob {
    let job = queue.next_job().expect("a queued column");
    queue.mark_running(job);
    job
}

#[test]
fn marking_the_same_column_twice_yields_one_job() {
    let mut queue = MeshQueue::new();
    queue.mark_dirty(3, -4);
    queue.mark_dirty(3, -4);
    assert_eq!(queue.pending(), 1, "one column, however often it is marked");
    let job = take(&mut queue);
    assert_eq!((job.cx, job.cz), (3, -4));
    assert!(
        queue.complete(job),
        "the completion matches the generation the job carried"
    );
    assert_eq!(queue.pending(), 0, "a fresh completion drains the queue");
    assert_eq!(queue.next_job(), None, "nothing is left to build");
}

#[test]
fn a_change_between_the_job_and_its_completion_makes_it_stale() {
    let mut queue = MeshQueue::new();
    queue.mark_dirty(1, 2);
    let job = take(&mut queue);
    // The column changes while the job is out: the snapshot the job took is
    // already out of date.
    queue.mark_dirty(1, 2);
    assert!(!queue.complete(job), "the stale completion is discarded");
    assert_eq!(queue.pending(), 1, "the column is queued again");
    let next = take(&mut queue);
    assert!(
        next.generation > job.generation,
        "the rebuild carries the change: {} against {}",
        next.generation,
        job.generation
    );
    assert!(queue.complete(next));
    assert_eq!(queue.pending(), 0);
}

#[test]
fn a_second_mark_between_next_job_and_complete_is_stale() {
    let mut queue = MeshQueue::new();
    queue.mark_dirty(6, 6);
    let job = queue.next_job().expect("the column is queued");
    // The mark lands before the job was even recorded as running.
    queue.mark_dirty(6, 6);
    queue.mark_running(job);
    assert!(!queue.complete(job), "the second mark outdates the job");
    let next = take(&mut queue);
    assert!(next.generation > job.generation);
    assert!(queue.complete(next));
    assert_eq!(queue.pending(), 0);
}

#[test]
fn a_stale_completion_requeues_the_column_exactly_once() {
    let mut queue = MeshQueue::new();
    queue.mark_dirty(5, 5);
    let job = take(&mut queue);
    // Two changes while the job is out still leave one queued column.
    queue.mark_dirty(5, 5);
    queue.mark_dirty(5, 5);
    assert!(!queue.complete(job));
    assert_eq!(
        queue.pending(),
        1,
        "the column is queued once, not once per mark"
    );
    let next = take(&mut queue);
    assert_eq!(
        next.generation, 3,
        "the queued job carries the latest generation"
    );
    assert!(queue.complete(next));
}

#[test]
fn mark_running_keeps_a_column_out_of_a_second_job() {
    let mut queue = MeshQueue::new();
    queue.mark_dirty(0, 0);
    queue.mark_dirty(7, 7);
    let first = take(&mut queue);
    assert_eq!((first.cx, first.cz), (0, 0), "jobs come in mark order");
    let second = queue.next_job().expect("the other column is still queued");
    assert_eq!(
        (second.cx, second.cz),
        (7, 7),
        "the running column is not handed out again"
    );
    queue.mark_running(second);
    assert_eq!(queue.next_job(), None, "every queued column has a job out");
    assert_eq!(queue.pending(), 2, "both columns are outstanding");
    assert!(queue.complete(first) && queue.complete(second));
    assert_eq!(queue.pending(), 0);
}

#[test]
fn an_unload_queues_the_four_neighbours_and_drops_the_column() {
    let mut queue = MeshQueue::new();
    queue.mark_dirty(2, 3);
    queue.mark_column_unloaded(2, 3);
    assert_eq!(
        queue.pending(),
        4,
        "the four neighbours, not the column itself"
    );
    let mut queued = Vec::new();
    while let Some(job) = queue.next_job() {
        queue.mark_running(job);
        queued.push((job.cx, job.cz));
    }
    queued.sort_unstable();
    assert_eq!(queued, vec![(1, 3), (2, 2), (2, 4), (3, 3)]);
    assert!(
        !queue.generations().contains_key(&(2, 3)),
        "the unloaded column is forgotten"
    );
}

#[test]
fn an_unload_requeues_an_in_flight_column_for_a_last_build() {
    let mut queue = MeshQueue::new();
    queue.mark_dirty(4, 4);
    let job = take(&mut queue);
    queue.mark_column_unloaded(4, 4);
    assert!(
        !queue.complete(job),
        "the build is for a column that is gone"
    );
    assert_eq!(
        queue.pending(),
        5,
        "the four neighbours and one last build of the removed column"
    );
}

#[test]
fn every_change_bumps_the_column_generation() {
    let mut queue = MeshQueue::new();
    queue.mark_dirty(9, 9);
    assert_eq!(queue.generations().get(&(9, 9)), Some(&1));
    queue.mark_dirty(9, 9);
    assert_eq!(queue.generations().get(&(9, 9)), Some(&2));
    let job = take(&mut queue);
    assert_eq!(
        job.generation, 2,
        "the job carries the generation it was handed at"
    );
    queue.mark_dirty(9, 9);
    assert_eq!(
        queue.generations().get(&(9, 9)),
        Some(&3),
        "a running column's generation still bumps"
    );
    assert!(!queue.complete(job));
    let next = take(&mut queue);
    assert_eq!(next.generation, 3);
}
