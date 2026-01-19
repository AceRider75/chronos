use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::arch::x86_64::_rdtsc;
use crate::writer;

// A "Job" is just a function pointer.
pub type Job = fn();

pub struct Task {
    pub name: String,
    pub budget: u64,
    pub job: Job,
    pub last_cost: u64, // How long it took last time
    pub status: TaskStatus,
}

#[derive(PartialEq)]
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

    // Add a new task to the list
    pub fn add_task(&mut self, name: &str, budget: u64, job: Job) {
        self.tasks.push(Task {
            name: String::from(name),
            budget,
            job,
            last_cost: 0,
            status: TaskStatus::Waiting,
        });
    }

    // THE EXECUTION LOOP
    pub fn execute_frame(&mut self) {
        let mut total_time: u64 = 0;

        for task in self.tasks.iter_mut() {
            // 1. START CLOCK
            let start = unsafe { _rdtsc() };

            // 2. RUN TASK
            (task.job)();

            // 3. STOP CLOCK
            let end = unsafe { _rdtsc() };
            let cost = end - start;
            
            task.last_cost = cost;
            total_time += cost;

            // 4. JUDGE TASK
            if cost <= task.budget {
                task.status = TaskStatus::Success;
            } else {
                task.status = TaskStatus::Failure;
            }
        }
    }

    // Visualize the results on screen
    pub fn draw_debug(&self) {
        // Reset cursor to below the main headers
        // We cheat a bit by accessing the global writer lock directly here 
        // or just rely on the main loop to position the cursor, 
        // but let's just print a status list.
        
        // Let's print to a specific Y location (handled by the writer if we added set_cursor)
        // For now, we just dump the list.
        
        for task in &self.tasks {
            let status_icon = match task.status {
                TaskStatus::Waiting => "[....]",
                TaskStatus::Success => "[ PASS ]",
                TaskStatus::Failure => "[ FAIL ]",
            };
            
            // Format: [ PASS ] TaskName (Cost / Budget)
            let msg = format!("{} {} ({}/{})\n", 
                status_icon, 
                task.name, 
                task.last_cost, 
                task.budget
            );
            
            writer::print(&msg);
        }
        writer::print("--------------------------\n");
    }
}