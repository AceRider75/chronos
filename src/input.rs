use alloc::collections::vec_deque::VecDeque;
use spin::Mutex;
use lazy_static::lazy_static;

// A Queue of characters (FIFO)
lazy_static! {
    pub static ref KEYBOARD_BUFFER: Mutex<VecDeque<char>> = Mutex::new(VecDeque::new());
}

// Helper to push a key
pub fn push_key(c: char) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut buffer = KEYBOARD_BUFFER.lock();
        buffer.push_back(c);
    });
}

// Helper to pop a key
pub fn pop_key() -> Option<char> {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut buffer = KEYBOARD_BUFFER.lock();
        buffer.pop_front()
    })
}