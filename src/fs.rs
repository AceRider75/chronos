use limine::request::ModuleRequest;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
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

pub fn init() {
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