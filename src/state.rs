use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub static CYCLE_BUDGET: AtomicU64 = AtomicU64::new(2_500_000);
pub static KEY_COUNT: AtomicU64 = AtomicU64::new(0);
pub static HHDM_OFFSET: AtomicU64 = AtomicU64::new(0);
pub static KERNEL_DELTA: AtomicU64 = AtomicU64::new(0);

// NEW: Store our IP address (Default 0.0.0.0)
// We use U32 to store 4 bytes of IP
pub static MY_IP: AtomicU32 = AtomicU32::new(0);

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