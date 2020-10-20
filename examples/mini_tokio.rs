use crossbeam::channel;
use futures::task::{self, ArcWake};
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    let mini_tokio = MiniTokio::new();

    // Spawn root task.
    // No work happens until `mini_tokio.run()` is called.
    mini_tokio.spawn(async {
        spawn(async {
            Delay::with(Duration::from_millis(100)).await;
            println!("world");
        });

        spawn(async {
            println!("hello");
        });

        Delay::with(Duration::from_millis(200)).await;
        std::process::exit(0);
    });

    mini_tokio.run();
}

struct MiniTokio {
    // Receives scheduled tasks. When a task is scheduled, the associated future is ready to make progress.
    // This usually happens when a resource the task uses becomes ready to perform an operation.
    // For example, a socket received data and read call will succeed.
    scheduled: channel::Receiver<Arc<Task>>,

    // Send half ot the scheduled channel.
    sender: channel::Sender<Arc<Task>>,
}

impl MiniTokio {
    fn new() -> MiniTokio {
        let (sender, scheduled) = channel::unbounded();

        MiniTokio { scheduled, sender }
    }

    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Task::spawn(future, &self.sender);
    }

    fn run(&self) {
        CURRENT.with(|cell| {
            *cell.borrow_mut() = Some(self.sender.clone());
        });

        while let Ok(task) = self.scheduled.recv() {
            task.poll();
        }
    }
}

pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    CURRENT.with(|cell| {
        let borrow = cell.borrow();
        let sender = borrow.as_ref().unwrap();
        Task::spawn(future, sender);
    });
}

struct Delay {
    // When to complete the delay.
    when: Instant,
    // The waker to notify once the delay has completed.
    // The waker must be accessible by both the timer thread and future so it is wrapped with `Arc<Mutex>>`
    waker: Option<Arc<Mutex<Waker>>>,
}

impl Future for Delay {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // First, if this is the first time the future is called, spawn the timer thread.
        // If the timer thread is already running, ensure the stored `Waker` matches the current task's waker.
        if let Some(waker) = &self.waker {
            let mut waker = waker.lock().unwrap();

            // Check if the stored waker matches the current tasks' waker.
            if !waker.will_wake(cx.waker()) {
                *waker = cx.waker().clone();
            }
        } else {
            let when = self.when;
            let waker = Arc::new(Mutex::new(cx.waker().clone()));
            self.waker = Some(waker.clone());

            // This is the first time `poll` is called, spawn the timer thread.
            thread::spawn(move || {
                let now = Instant::now();

                if now < when {
                    thread::sleep(when - now);
                }

                let waker = waker.lock().unwrap();
                waker.wake_by_ref();
            });
        }

        if Instant::now() >= self.when {
            Poll::Ready(())
        } else {
            // The duration has not elapsed, the future has not completed so return `Poll::Pending`.
            Poll::Pending
        }
    }
}

impl Delay {
    async fn with(dur: Duration) {
        let future = Delay {
            when: Instant::now() + dur,
            waker: None,
        };

        future.await;
    }
}

thread_local! {
    static CURRENT: RefCell<Option<channel::Sender<Arc<Task>>>> = RefCell::new(None);
}

struct Task {
    future: Mutex<Pin<Box<dyn Future<Output = ()> + Send>>>,
    executor: channel::Sender<Arc<Task>>,
}

impl Task {
    fn spawn<F>(future: F, sender: &channel::Sender<Arc<Task>>)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let task = Arc::new(Task {
            future: Mutex::new(Box::pin(future)),
            executor: sender.clone(),
        });

        sender.send(task).unwrap();
        //let _ = sender.send(task);
    }

    fn poll(self: Arc<Self>) {
        let waker = task::waker(self.clone());
        let mut cx = Context::from_waker(&waker);

        let mut future = self.future.try_lock().unwrap();

        let _ = future.as_mut().poll(&mut cx);
    }
}

impl ArcWake for Task {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        let _ = arc_self.executor.send(arc_self.clone());
    }
}
