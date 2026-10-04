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
fn a_forgotten_column_never_reuses_a_generation() {
    // The column is unloaded while a job is out, then sent again. The build
    // that was out when the column vanished took its snapshot before the
    // unload; it must not be able to answer as the re-created column's
    // current build, so the new job is handed a generation no job has carried.
    let mut queue = MeshQueue::new();
    queue.mark_dirty(8, 8);
    let stale = take(&mut queue);
    queue.mark_column_unloaded(8, 8);
    queue.mark_dirty(8, 8);
    let mut fresh = None;
    while let Some(job) = queue.next_job() {
        queue.mark_running(job);
        if (job.cx, job.cz) == (8, 8) {
            fresh = Some(job);
        }
    }
    let fresh = fresh.expect("the re-created column is queued");
    assert!(
        fresh.generation > stale.generation,
        "the re-created column's job carries a new generation: {} against {}",
        fresh.generation,
        stale.generation
    );
    assert!(
        !queue.complete(stale),
        "the pre-unload build is discarded, not reported fresh"
    );
    assert!(
        queue.complete(fresh),
        "the rebuild is the column's current build"
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

#[test]
fn a_clear_empties_the_queue_and_keeps_the_generation_counter() {
    // The state backlog's item 2: a world reset empties the queue in place,
    // and the generations the old world's jobs carried must never come round
    // again, or a build still out across the reset could answer as the new
    // world's own.
    let mut queue = MeshQueue::new();
    queue.mark_dirty(0, 0);
    queue.mark_dirty(1, 1);
    let stale = take(&mut queue);
    assert_eq!(stale.generation, 1, "the counter starts at one");
    queue.clear_in_place();
    assert_eq!(queue.pending(), 0, "the clear empties the queue");
    assert_eq!(queue.next_job(), None, "nothing is left to build");
    assert!(queue.generations().is_empty(), "no column is outstanding");

    queue.mark_dirty(5, 5);
    let fresh = take(&mut queue);
    assert_eq!(
        fresh.generation, 3,
        "the counter survives the clear and keeps counting: one per mark, nothing reset"
    );
    assert!(
        fresh.generation > stale.generation,
        "no generation is ever handed out twice: {} against {}",
        fresh.generation,
        stale.generation
    );
}

#[test]
fn a_build_out_across_the_clear_is_discarded_and_new_work_is_kept() {
    // The ordering guarantee `clear_in_place` exists for: a mesh result
    // handed out before the clear and returned after it is rejected as stale;
    // work enqueued after the clear is accepted.
    let mut queue = MeshQueue::new();
    queue.mark_dirty(2, 2);
    let stale = take(&mut queue);
    queue.clear_in_place();
    queue.mark_dirty(2, 2);
    let fresh = take(&mut queue);
    assert!(
        fresh.generation > stale.generation,
        "the rebuild carries a generation the old job cannot match"
    );
    assert!(
        !queue.complete(stale),
        "the pre-clear build is discarded, not reported as the new world's"
    );
    assert!(
        queue.complete(fresh),
        "the post-clear build is the column's current one"
    );
    assert_eq!(queue.pending(), 0, "a fresh completion drains the queue");
}

#[test]
fn a_stale_build_landing_before_the_rebuild_leaves_the_column_queued_once() {
    // The other return order: the pre-clear build comes back after the clear
    // re-queued the column but before the rebuild was handed out. It is
    // discarded and the column keeps its one place in the queue.
    let mut queue = MeshQueue::new();
    queue.mark_dirty(3, 3);
    let stale = take(&mut queue);
    queue.clear_in_place();
    queue.mark_dirty(3, 3);
    assert!(!queue.complete(stale), "the pre-clear build is discarded");
    assert_eq!(
        queue.pending(),
        1,
        "the column is queued once, not once per mark"
    );
    let fresh = take(&mut queue);
    assert!(queue.complete(fresh));
    assert_eq!(queue.pending(), 0);
}
