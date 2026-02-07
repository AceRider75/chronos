use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::arch::x86_64::_rdtsc;
use spin::Mutex;
use lazy_static::lazy_static;

pub type Job = extern "C" fn(u64);

fn task_exit() {
    unsafe {
        core::arch::asm!(
            "int 0x80",
            in("rax") 2, // exit
        );
    }
    loop { core::hint::spin_loop(); }
}

pub static mut SCHEDULER_CONTEXT: TaskContext = TaskContext {
    r15: 0, r14: 0, r13: 0, r12: 0, r11: 0, r10: 0, r9: 0, r8: 0,
    rbp: 0, rdi: 0, rsi: 0, rdx: 0, rcx: 0, rbx: 0, rax: 0,
    rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
};

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskContext {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    
    // Pushed by IRETQ
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

pub struct Task {
    pub name: String,
    pub budget: u64,
    pub job: Job,
    pub last_cost: u64,
    pub status: TaskStatus,
    pub violation_count: u32,
    pub penalty_cooldown: u32,
    pub context: TaskContext,
    pub stack: Vec<u8>,
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
    pub current_task_idx: Option<usize>,
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler {
            tasks: Vec::new(),
            current_task_idx: None,
        }
    }

    pub fn add_task(&mut self, name: &str, budget: u64, job: Job, arg: u64) {
        let mut stack = alloc::vec![0u8; 65536];
        let stack_ptr = stack.as_ptr() as u64 + 65536;
        
        // Push task_exit to stack so tasks can 'return'
        unsafe {
            let stack_top = (stack_ptr - 8) as *mut u64;
            *stack_top = task_exit as *const () as u64;
        }

        let mut context = TaskContext::default();
        context.rip = job as u64;
        context.rdi = arg; // Pass argument in RDI (System V ABI)
        context.rsp = stack_ptr - 8;
        context.cs = 0x8; // Kernel Code Selector
        context.ss = 0x10; // Kernel Data Selector
        context.rflags = 0x202; // Interrupts enabled

        self.tasks.push(Task {
            name: String::from(name),
            budget,
            job,
            last_cost: 0,
            status: TaskStatus::Waiting,
            violation_count: 0,
            penalty_cooldown: 0,
            context,
            stack,
        });
    }

    pub fn execute_frame(&mut self) {
        // Obsolete: Use scheduler::step() instead
    }
}

static mut NEXT_TASK_IDX: usize = 0;

pub fn step() {
    let mut task_idx = None;
    
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        if sched.tasks.is_empty() { return; }
        
        let mut i = unsafe { NEXT_TASK_IDX } % sched.tasks.len();
        
        // Find next non-penalized task
        let start_i = i;
        loop {
            if sched.tasks[i].penalty_cooldown == 0 {
                task_idx = Some(i);
                break;
            }
            sched.tasks[i].penalty_cooldown -= 1;
            sched.tasks[i].status = TaskStatus::Penalty;
            i = (i + 1) % sched.tasks.len();
            if i == start_i { break; }
        }
        
        if let Some(idx) = task_idx {
            sched.current_task_idx = Some(idx);
            unsafe { NEXT_TASK_IDX = (idx + 1) % sched.tasks.len(); }
        }
    });

    if let Some(idx) = task_idx {
        let start = unsafe { _rdtsc() };

        // 1. Copy context to load to a local variable to avoid pointer-into-Vec issues
        let context_to_load = x86_64::instructions::interrupts::without_interrupts(|| {
            let sched = SCHEDULER.lock();
            sched.tasks[idx].context
        });
        
        // 2. Switch must be atomic w.r.t the saving into SCHEDULER_CONTEXT
        unsafe {
            x86_64::instructions::interrupts::disable();
            context_switch(&mut SCHEDULER_CONTEXT, &context_to_load as *const TaskContext);
            x86_64::instructions::interrupts::enable();
        }
        
        let end = unsafe { _rdtsc() };
        
        x86_64::instructions::interrupts::without_interrupts(|| {
            let mut sched = SCHEDULER.lock();
            sched.current_task_idx = None;
            if idx < sched.tasks.len() {
                sched.tasks[idx].last_cost = end - start;
                // Enforce Contract
                if sched.tasks[idx].last_cost <= sched.tasks[idx].budget {
                    sched.tasks[idx].status = TaskStatus::Success;
                    if sched.tasks[idx].violation_count > 0 { sched.tasks[idx].violation_count -= 1; }
                } else {
                    sched.tasks[idx].status = TaskStatus::Failure;
                    sched.tasks[idx].violation_count += 1;
                    if sched.tasks[idx].violation_count >= 3 {
                        sched.tasks[idx].penalty_cooldown = 5;
                        sched.tasks[idx].violation_count = 0;
                    }
                }
            }
        });
    }
}



#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(save: *mut TaskContext, load: *const TaskContext) {
    core::arch::naked_asm!(
        // 1. Save all registers and RFLAGS to stack
        "pushfq",
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        
        // 2. Copy from stack to 'save' (rdi)
        "mov rax, rdi",
        "pop rbx", "mov [rax + 0], rbx",   // r15
        "pop rbx", "mov [rax + 8], rbx",   // r14
        "pop rbx", "mov [rax + 16], rbx",  // r13
        "pop rbx", "mov [rax + 24], rbx",  // r12
        "pop rbx", "mov [rax + 32], rbx",  // r11
        "pop rbx", "mov [rax + 40], rbx",  // r10
        "pop rbx", "mov [rax + 48], rbx",  // r9
        "pop rbx", "mov [rax + 56], rbx",  // r8
        "pop rbx", "mov [rax + 64], rbx",  // rbp
        "pop rbx", "mov [rax + 72], rbx",  // rdi
        "pop rbx", "mov [rax + 80], rbx",  // rsi
        "pop rbx", "mov [rax + 88], rbx",  // rdx
        "pop rbx", "mov [rax + 96], rbx",  // rcx
        "pop rbx", "mov [rax + 104], rbx", // rbx
        "pop rbx", "mov [rax + 112], rbx", // rax
        
        // Stack now has: [rflags], [return_address]
        "pop rbx", // rbx = rflags
        "or rbx, 0x200", // Force IF bit to ensure interrupts are enabled when restored
        "mov [rax + 136], rbx", // rflags
        
        "pop rbx", // rbx = return address (rip)
        "mov [rax + 120], rbx", // rip
        
        "mov rbx, cs",
        "mov [rax + 128], rbx",
        "mov [rax + 144], rsp", // rsp
        "mov rbx, ss",
        "mov [rax + 152], rbx",
        
        "cli",
        
        // 3. Load from 'load' (rsi)
        "mov r15, [rsi + 0]",
        "mov r14, [rsi + 8]",
        "mov r13, [rsi + 16]",
        "mov r12, [rsi + 24]",
        "mov r11, [rsi + 32]",
        "mov r10, [rsi + 40]",
        "mov r9, [rsi + 48]",
        "mov r8, [rsi + 56]",
        "mov rbp, [rsi + 64]",
        "mov rdi, [rsi + 72]",
        "mov rdx, [rsi + 88]",
        "mov rcx, [rsi + 96]",
        "mov rbx, [rsi + 104]",
        "mov rax, [rsi + 112]",
        
        // Prepare IRETQ frame
        "push [rsi + 152]", // ss
        "push [rsi + 144]", // rsp
        "push [rsi + 136]", // rflags
        "push [rsi + 128]", // cs
        "push [rsi + 120]", // rip
        
        "mov rsi, [rsi + 80]",
        "iretq",
    );
}

// --- GLOBAL INSTANCE ---
lazy_static! {
    pub static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}