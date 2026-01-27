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
    pub violation_count: u32,
    pub penalty_cooldown: u32,
}

#[derive(PartialEq, Clone, Copy)]
pub enum TaskStatus {
    Waiting,
    Success,
    Failure,
    Penalty,
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
            violation_count: 0,
            penalty_cooldown: 0,
        });
    }

    pub fn execute_frame(&mut self) {
        for task in self.tasks.iter_mut() {
            // 1. Check Penalty Box
            if task.penalty_cooldown > 0 {
                task.penalty_cooldown -= 1;
                task.status = TaskStatus::Penalty;
                continue;
            }

            // 2. Execute Task
            let start = unsafe { _rdtsc() };
            (task.job)();
            let end = unsafe { _rdtsc() };
            
            task.last_cost = end - start;

            // 3. Enforce Contract
            if task.last_cost <= task.budget {
                task.status = TaskStatus::Success;
                if task.violation_count > 0 { task.violation_count -= 1; }
            } else {
                task.status = TaskStatus::Failure;
                task.violation_count += 1;
                
                // Penalty Box: If you fail 3 times, you are benched for 5 frames
                if task.violation_count >= 3 {
                    task.penalty_cooldown = 5;
                    task.violation_count = 0;
                }
            }
        }
    }
}

// --- GLOBAL INSTANCE ---
lazy_static! {
    pub static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}