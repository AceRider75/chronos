use linked_list_allocator::LockedHeap;
use x86_64::structures::paging::mapper::MapToError;
use x86_64::VirtAddr;

// 1. DEFINE THE HEAP
// We create a wrapper around the allocator that is thread-safe (LockedHeap).
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

// 2. DEFINE THE MEMORY REGION
// Instead of scanning RAM, we reserve a big chunk of memory 
// inside our own kernel binary to act as the heap.
// 32 MiB size.
pub const HEAP_SIZE: usize = 32 * 1024 * 1024; 

// We use 'static mut' to allocate space in the BSS section.
// This is effectively a big array of zero bytes.
static mut HEAP_MEM: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

// 3. INITIALIZE
pub fn init_heap() {
    unsafe {
        // Tell the allocator to use our static array as the heap source.
        let heap_start = HEAP_MEM.as_ptr() as usize;
        ALLOCATOR.lock().init(heap_start as *mut u8, HEAP_SIZE);
    }
}

pub fn get_heap_usage() -> (usize, usize) {
    let heap = ALLOCATOR.lock();
    (heap.used(), heap.size())
}

// 4. ERROR HANDLING
// If we run out of memory, this function is called.
#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}