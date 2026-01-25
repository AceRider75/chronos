#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Registers {
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
    // The CPU pushes RIP, CS, RFLAGS, RSP, SS automatically on interrupt
    // We will point to those using the Stack Pointer
}

#[derive(Clone)]
pub struct Process {
    pub id: usize,
    pub name: alloc::string::String,
    pub stack_pointer: u64, // The "Saved Place" in this task
    pub state: ProcessState,
    pub page_table_phys: u64, // CR3 for this process
}

#[derive(Clone, PartialEq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
}