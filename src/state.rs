

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering}; // Added AtomicUsize

// ... existing vars ...
pub static CYCLE_BUDGET: AtomicU64 = AtomicU64::new(2_500_000);
pub static KEY_COUNT: AtomicU64 = AtomicU64::new(0);
pub static HHDM_OFFSET: AtomicU64 = AtomicU64::new(0);
pub static KERNEL_DELTA: AtomicU64 = AtomicU64::new(0);
pub static MY_IP: AtomicU32 = AtomicU32::new(0);

// Video State
pub static VIDEO_PTR: AtomicU64 = AtomicU64::new(0);
pub static SCREEN_WIDTH: AtomicUsize = AtomicUsize::new(1024); // Default
pub static SCREEN_HEIGHT: AtomicUsize = AtomicUsize::new(768);

pub fn set_my_ip(ip: [u8; 4]) {
    let combined = ((ip[0] as u32) << 24) | ((ip[1] as u32) << 16) | ((ip[2] as u32) << 8) | (ip[3] as u32);
    MY_IP.store(combined, Ordering::Relaxed);
}

pub fn get_my_ip() -> [u8; 4] {
    let combined = MY_IP.load(Ordering::Relaxed);
    [
        (combined >> 24) as u8,
        (combined >> 16) as u8,
        (combined >> 8) as u8,
        combined as u8,
    ]
}

pub fn adjust_budget(amount: i64) {
    let current = CYCLE_BUDGET.load(Ordering::Relaxed);
    let new_val = if amount < 0 {
        if current > 500_000 { current - (amount.abs() as u64) } else { current }
    } else {
        current + (amount as u64)
    };
    CYCLE_BUDGET.store(new_val, Ordering::Relaxed);
}