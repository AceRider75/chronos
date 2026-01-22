use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::arch::x86_64::_rdtsc;
use spin::Mutex;
use lazy_static::lazy_static;

pub type Job = fn();

pub struct Task {
    pub name: String,
    pub budget: u64,
    pub job: Job,
    pub last_cost: u64,
    pub status: TaskStatus,
}

#[derive(PartialEq, Clone, Copy)]
pub enum TaskStatus {
    Waiting,
    Success,
    Failure,
}

pub struct Scheduler {
    pub tasks: Vec<Task>,
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler {
            tasks: Vec::new(),
        }
    }

    pub fn add_task(&mut self, name: &str, budget: u64, job: Job) {
        self.tasks.push(Task {
            name: String::from(name),
            budget,
            job,
            last_cost: 0,
            status: TaskStatus::Waiting,
        });
    }

    pub fn execute_frame(&mut self) {
        for task in self.tasks.iter_mut() {
            let start = unsafe { _rdtsc() };
            (task.job)();
            let end = unsafe { _rdtsc() };
            
            task.last_cost = end - start;

            if task.last_cost <= task.budget {
                task.status = TaskStatus::Success;
            } else {
                task.status = TaskStatus::Failure;
            }
        }
    }
}

// --- GLOBAL INSTANCE ---
lazy_static! {
    pub static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}