use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    // A queue to hold messages from drivers until the Shell is ready to print them
    pub static ref LOG_QUEUE: Mutex<Vec<String>> = Mutex::new(Vec::new());
}

// Drivers call this instead of printing directly
pub fn log(msg: &str) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut queue = LOG_QUEUE.lock();
        // Prevent infinite memory growth: Keep last 50 messages
        if queue.len() > 50 {
            queue.remove(0);
        }
        queue.push(String::from(msg));
    });
}

// The Shell calls this to get new messages
pub fn drain() -> Vec<String> {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut queue = LOG_QUEUE.lock();
        let items = queue.clone();
        queue.clear();
        items
    })
}