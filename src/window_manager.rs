use crate::compositor::Window;
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref WINDOWS: Mutex<Vec<Window>> = Mutex::new(Vec::new());
    pub static ref ACTIVE_WINDOW: Mutex<usize> = Mutex::new(0);
}

pub fn add_window(win: Window) {
    WINDOWS.lock().push(win);
}