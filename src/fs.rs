use limine::request::ModuleRequest;
use limine::BaseRevision;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

// Request the list of loaded modules from the bootloader
#[used]
static MODULE_REQUEST: ModuleRequest = ModuleRequest::new();

pub struct File {
    pub name: String,
    pub data: &'static [u8],
}

// 1. List all files found in RAM
pub fn list_files() -> Vec<File> {
    let mut files = Vec::new();

    if let Some(response) = MODULE_REQUEST.get_response() {
        for module in response.modules() {
            // Get raw pointer and size
            let start = module.addr() as *const u8;
            let size = module.size() as usize;

            // Create a Rust slice from the raw memory
            let data = unsafe { core::slice::from_raw_parts(start, size) };

            // FIX: Convert CStr directly to &str
            let path_cstr = module.path(); 
            let path_str = path_cstr.to_str().unwrap_or("unknown");
            
            files.push(File {
                name: path_str.to_string(),
                data,
            });
        }
    }
    files
}

// 2. Read a specific file by name
pub fn read_file(query: &str) -> Option<String> {
    let files = list_files();
    for file in files {
        if file.name.contains(query) {
            return core::str::from_utf8(file.data)
                .ok()
                .map(|s| s.to_string());
        }
    }
    None
}