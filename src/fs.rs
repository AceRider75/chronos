use crate::writer;
use limine::request::ModuleRequest;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::format;
use spin::Mutex;
use lazy_static::lazy_static;

#[used]
static MODULE_REQUEST: ModuleRequest = ModuleRequest::new();

#[derive(Clone)]
pub enum Node {
    File { name: String, data: Vec<u8> },
    Directory { name: String, children: Vec<Node> },
}

impl Node {
    pub fn name(&self) -> &str {
        match self {
            Node::File { name, .. } => name,
            Node::Directory { name, .. } => name,
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, Node::Directory { .. })
    }
}

lazy_static! {
    pub static ref ROOT: Mutex<Node> = Mutex::new(Node::Directory {
        name: "/".to_string(),
        children: Vec::new(),
    });
}

// Helper to find a directory by path (simple absolute path for now)
pub fn find_dir_mut<'a>(root: &'a mut Node, path: &str) -> Option<&'a mut Node> {
    if path == "/" || path == "" {
        return Some(root);
    }

    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut current = root;

    for part in parts {
        if let Node::Directory { children, .. } = current {
            let mut found_idx = None;
            for (i, child) in children.iter().enumerate() {
                if child.name() == part && child.is_dir() {
                    found_idx = Some(i);
                    break;
                }
            }
            if let Some(idx) = found_idx {
                current = &mut children[idx];
            } else {
                return None;
            }
        } else {
            return None;
        }
    }
    Some(current)
}

pub fn mkdir(path: &str, name: &str) -> bool {
    let mut root = ROOT.lock();
    if let Some(dir) = find_dir_mut(&mut root, path) {
        if let Node::Directory { children, .. } = dir {
            if children.iter().any(|c| c.name() == name) {
                return false;
            }
            children.push(Node::Directory {
                name: name.to_string(),
                children: Vec::new(),
            });
            return true;
        }
    }
    false
}

pub fn touch(path: &str, name: &str, data: Vec<u8>) -> bool {
    let mut root = ROOT.lock();
    if let Some(dir) = find_dir_mut(&mut root, path) {
        if let Node::Directory { children, .. } = dir {
            if let Some(pos) = children.iter().position(|c| c.name() == name) {
                children[pos] = Node::File { name: name.to_string(), data };
            } else {
                children.push(Node::File { name: name.to_string(), data });
            }
            return true;
        }
    }
    false
}

pub fn rm(path: &str, name: &str) -> bool {
    let mut root = ROOT.lock();
    if let Some(dir) = find_dir_mut(&mut root, path) {
        if let Node::Directory { children, .. } = dir {
            if let Some(pos) = children.iter().position(|c| c.name() == name) {
                children.remove(pos);
                return true;
            }
        }
    }
    false
}

pub fn ls(path: &str) -> Option<Vec<(String, bool)>> {
    let mut root = ROOT.lock();
    if let Some(dir) = find_dir_mut(&mut root, path) {
        if let Node::Directory { children, .. } = dir {
            return Some(children.iter().map(|c| (c.name().to_string(), c.is_dir())).collect());
        }
    }
    None
}

pub fn read(path: &str, name: &str) -> Option<Vec<u8>> {
    let mut root = ROOT.lock();
    if let Some(dir) = find_dir_mut(&mut root, path) {
        if let Node::Directory { children, .. } = dir {
            for child in children {
                if let Node::File { name: n, data } = child {
                    if n == name {
                        return Some(data.clone());
                    }
                }
            }
        }
    }
    None
}

// --- NEW CORE FUNCTIONS ---

pub fn copy_node(src_path: &str, src_name: &str, dest_path: &str, dest_name: &str) -> bool {
    let mut root = ROOT.lock();
    
    // 1. Get source node
    let src_node = {
        if let Some(dir) = find_dir_mut(&mut root, src_path) {
            if let Node::Directory { children, .. } = dir {
                if let Some(node) = children.iter().find(|c| c.name() == src_name) {
                    node.clone()
                } else {
                    return false;
                }
            } else {
                return false;
            }
        } else {
            return false;
        }
    };

    // 2. Rename if needed
    let mut new_node = src_node;
    match &mut new_node {
        Node::File { name, .. } => *name = dest_name.to_string(),
        Node::Directory { name, .. } => *name = dest_name.to_string(),
    }

    // 3. Place in destination
    if let Some(dest_dir) = find_dir_mut(&mut root, dest_path) {
        if let Node::Directory { children, .. } = dest_dir {
            // Remove existing if any
            if let Some(pos) = children.iter().position(|c| c.name() == dest_name) {
                children.remove(pos);
            }
            children.push(new_node);
            return true;
        }
    }
    false
}

pub fn move_node(src_path: &str, src_name: &str, dest_path: &str, dest_name: &str) -> bool {
    let mut root = ROOT.lock();
    
    // 1. Remove source node
    let mut src_node = {
        if let Some(dir) = find_dir_mut(&mut root, src_path) {
            if let Node::Directory { children, .. } = dir {
                if let Some(pos) = children.iter().position(|c| c.name() == src_name) {
                    children.remove(pos)
                } else {
                    return false;
                }
            } else {
                return false;
            }
        } else {
            return false;
        }
    };

    // 2. Rename
    match &mut src_node {
        Node::File { name, .. } => *name = dest_name.to_string(),
        Node::Directory { name, .. } => *name = dest_name.to_string(),
    }

    // 3. Place in destination
    if let Some(dest_dir) = find_dir_mut(&mut root, dest_path) {
        if let Node::Directory { children, .. } = dest_dir {
            if let Some(pos) = children.iter().position(|c| c.name() == dest_name) {
                children.remove(pos);
            }
            children.push(src_node);
            return true;
        }
    }
    false
}

pub struct NodeInfo {
    pub name: String,
    pub is_dir: bool,
    pub size: usize,
    pub child_count: usize,
}

pub fn get_node_info(path: &str, name: &str) -> Option<NodeInfo> {
    let mut root = ROOT.lock();
    let dir = find_dir_mut(&mut root, path)?;
    if let Node::Directory { children, .. } = dir {
        let node = children.iter().find(|c| c.name() == name)?;
        match node {
            Node::File { name, data } => Some(NodeInfo {
                name: name.clone(),
                is_dir: false,
                size: data.len(),
                child_count: 0,
            }),
            Node::Directory { name, children } => Some(NodeInfo {
                name: name.clone(),
                is_dir: true,
                size: 0, // Directories don't have "size" in this simple VFS
                child_count: children.len(),
            }),
        }
    } else {
        None
    }
}

pub fn walk_tree<F>(path: &str, mut callback: F) 
where F: FnMut(&str, &Node) {
    let mut root = ROOT.lock();
    if let Some(start_node) = find_dir_mut(&mut root, path) {
        walk_recursive(path, start_node, &mut callback);
    }
}

fn walk_recursive<F>(current_path: &str, node: &Node, callback: &mut F)
where F: FnMut(&str, &Node) {
    callback(current_path, node);
    if let Node::Directory { name: _, children } = node {
        for child in children {
            let next_path = if current_path == "/" {
                format!("/{}", child.name())
            } else {
                format!("{}/{}", current_path, child.name())
            };
            walk_recursive(&next_path, child, callback);
        }
    }
}


pub fn init() {
    // 1. Try to load from disk first
    if load_from_disk() {
        writer::print("[FS] Persistent VFS loaded from disk.\n");
        return;
    }

    // 2. Fallback to Limine modules
    writer::print("[FS] No persistent VFS found. Initializing from boot modules.\n");
    let mut root = ROOT.lock();
    
    if let Some(response) = MODULE_REQUEST.get_response() {
        for module in response.modules() {
            let start = module.addr() as *const u8;
            let size = module.size() as usize;
            let raw_data = unsafe { core::slice::from_raw_parts(start, size) };
            let data = raw_data.to_vec();

            let path_str = module.path().to_str().unwrap_or("unknown");
            let clean_name = path_str.rfind('/').map(|idx| &path_str[idx+1..]).unwrap_or(path_str);

            if let Node::Directory { children, .. } = &mut *root {
                children.push(Node::File {
                    name: clean_name.to_string(),
                    data,
                });
            }
        }
    }
}

const DISK_LBA_START: u32 = 10000;
const MAGIC: &[u8] = b"CHRONOSFS";

pub fn save_to_disk() {
    let root = ROOT.lock();
    let mut data = Vec::new();
    
    // Header
    data.extend_from_slice(MAGIC);
    data.extend_from_slice(&0u32.to_le_bytes()); // Placeholder for size
    data.push(1); // Version

    // Serialize tree
    serialize_node(&root, &mut data);

    // Update size
    let size = data.len() as u32;
    data[9..13].copy_from_slice(&size.to_le_bytes());

    // Pad to 512 bytes
    let padding = (512 - (data.len() % 512)) % 512;
    for _ in 0..padding { data.push(0); }

    let drive = crate::ata::AtaDrive::new(true);
    if drive.identify() {
        drive.write_sectors(DISK_LBA_START, &data);
    }
}

pub fn load_from_disk() -> bool {
    let drive = crate::ata::AtaDrive::new(true);
    if !drive.identify() { return false; }

    // Read header (first sector)
    let header = drive.read_sectors(DISK_LBA_START, 1);
    if header.len() < 14 || &header[0..9] != MAGIC {
        return false;
    }

    let total_size = u32::from_le_bytes(header[9..13].try_into().unwrap()) as usize;
    if total_size == 0 || total_size > 10 * 1024 * 1024 { // 10MB limit for safety
        return false;
    }

    // Read full data
    let sectors = ((total_size + 511) / 512) as u8;
    let full_data = drive.read_sectors(DISK_LBA_START, sectors);
    
    let mut offset = 14; // After Magic, Size, Version
    if let Some(new_root) = deserialize_node(&full_data, &mut offset) {
        let mut root = ROOT.lock();
        *root = new_root;
        return true;
    }
    
    false
}

fn serialize_node(node: &Node, data: &mut Vec<u8>) {
    match node {
        Node::File { name, data: file_data } => {
            data.push(0); // Type: File
            serialize_string(name, data);
            data.extend_from_slice(&(file_data.len() as u32).to_le_bytes());
            data.extend_from_slice(file_data);
        }
        Node::Directory { name, children } => {
            data.push(1); // Type: Directory
            serialize_string(name, data);
            data.extend_from_slice(&(children.len() as u32).to_le_bytes());
            for child in children {
                serialize_node(child, data);
            }
        }
    }
}

fn deserialize_node(data: &[u8], offset: &mut usize) -> Option<Node> {
    if *offset >= data.len() { return None; }
    let node_type = data[*offset];
    *offset += 1;

    let name = deserialize_string(data, offset)?;

    if node_type == 0 { // File
        if *offset + 4 > data.len() { return None; }
        let size = u32::from_le_bytes(data[*offset..*offset+4].try_into().unwrap()) as usize;
        *offset += 4;
        if *offset + size > data.len() { return None; }
        let file_data = data[*offset..*offset+size].to_vec();
        *offset += size;
        Some(Node::File { name, data: file_data })
    } else { // Directory
        if *offset + 4 > data.len() { return None; }
        let count = u32::from_le_bytes(data[*offset..*offset+4].try_into().unwrap()) as u32;
        *offset += 4;
        let mut children = Vec::new();
        for _ in 0..count {
            children.push(deserialize_node(data, offset)?);
        }
        Some(Node::Directory { name, children })
    }
}

fn serialize_string(s: &str, data: &mut Vec<u8>) {
    data.extend_from_slice(&(s.len() as u32).to_le_bytes());
    data.extend_from_slice(s.as_bytes());
}

fn deserialize_string(data: &[u8], offset: &mut usize) -> Option<String> {
    if *offset + 4 > data.len() { return None; }
    let len = u32::from_le_bytes(data[*offset..*offset+4].try_into().unwrap()) as usize;
    *offset += 4;
    if *offset + len > data.len() { return None; }
    let s = String::from_utf8(data[*offset..*offset+len].to_vec()).ok()?;
    *offset += len;
    Some(s)
}

// Compatibility for existing code
pub fn list_files() -> Vec<crate::fs::FileCompatibility> {
    let root = ROOT.lock();
    if let Node::Directory { children, .. } = &*root {
        children.iter().filter_map(|c| {
            if let Node::File { name, data } = c {
                Some(crate::fs::FileCompatibility { name: name.clone(), data: data.clone() })
            } else {
                None
            }
        }).collect()
    } else {
        Vec::new()
    }
}

#[derive(Clone)]
pub struct FileCompatibility {
    pub name: String,
    pub data: Vec<u8>,
}