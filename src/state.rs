use core::sync::atomic::{AtomicU64, Ordering};

pub static CYCLE_BUDGET: AtomicU64 = AtomicU64::new(2_500_000);
pub static KEY_COUNT: AtomicU64 = AtomicU64::new(0);

// NEW: Store the High Half Direct Map offset globally
pub static HHDM_OFFSET: AtomicU64 = AtomicU64::new(0);

pub fn adjust_budget(amount: i64) {
    let current = CYCLE_BUDGET.load(Ordering::Relaxed);
    let new_val = if amount < 0 {
        if current > 500_000 { current - (amount.abs() as u64) } else { current }
    } else {
        current + (amount as u64)
    };
    CYCLE_BUDGET.store(new_val, Ordering::Relaxed);
}