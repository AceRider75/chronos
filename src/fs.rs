use limine::request::ModuleRequest;
// We don't need MemoryMapRequest here, just ModuleRequest
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use spin::Mutex;
use lazy_static::lazy_static;

#[used]
static MODULE_REQUEST: ModuleRequest = ModuleRequest::new();

#[derive(Clone)]
pub struct File {
    pub name: String,
    pub data: Vec<u8>, // Mutable Data
}

// Global Mutable Filesystem
lazy_static! {
    pub static ref FILESYSTEM: Mutex<Vec<File>> = Mutex::new(Vec::new());
}

// 1. Initialize: Copy Limine modules into our mutable list
pub fn init() {
    let mut fs = FILESYSTEM.lock();
    
    if let Some(response) = MODULE_REQUEST.get_response() {
        for module in response.modules() {
            let start = module.addr() as *const u8;
            let size = module.size() as usize;
            
            // Create a Rust slice
            let raw_data = unsafe { core::slice::from_raw_parts(start, size) };
            
            // COPY data to Heap (Vec)
            let data = raw_data.to_vec();

            // FIX: Convert CStr to Rust String directly
            let path_cstr = module.path(); 
            let path_str = path_cstr.to_str().unwrap_or("unknown");
            
            // Clean up name (remove "boot:///")
            let clean_name = if let Some(idx) = path_str.rfind('/') {
                &path_str[idx+1..]
            } else {
                path_str
            };

            fs.push(File {
                name: clean_name.to_string(),
                data,
            });
        }
    }
}

// 2. List Files
pub fn list_files() -> Vec<File> {
    FILESYSTEM.lock().clone()
}

// 3. Read File
pub fn read_file(name: &str) -> Option<Vec<u8>> {
    let fs = FILESYSTEM.lock();
    for file in fs.iter() {
        if file.name == name {
            return Some(file.data.clone());
        }
    }
    None
}

// 4. Create File (Touch)
pub fn create_file(name: &str) {
    let mut fs = FILESYSTEM.lock();
    // Check if exists
    for file in fs.iter() {
        if file.name == name { return; }
    }
    fs.push(File {
        name: name.to_string(),
        data: Vec::new(),
    });
}

// 5. Delete File (Rm)
pub fn delete_file(name: &str) {
    let mut fs = FILESYSTEM.lock();
    if let Some(pos) = fs.iter().position(|x| x.name == name) {
        fs.remove(pos);
    }
}

// 6. Write to File (Append)
pub fn append_file(name: &str, data: &[u8]) -> bool {
    let mut fs = FILESYSTEM.lock();
    for file in fs.iter_mut() {
        if file.name == name {
            file.data.extend_from_slice(data);
            return true;
        }
    }
    false
}