use std::collections::HashMap;

pub const HACKER_DIR_SUFFIX: &str = "/.hackeros/hacker-lang";

pub fn merge_hash_maps(dest: &mut HashMap<String, ()>, mut src: HashMap<String, ()>) {
    dest.extend(src.drain());
}

pub fn merge_string_hash_maps(dest: &mut HashMap<String, String>, mut src: HashMap<String, String>) {
    dest.extend(src.drain());
}

pub fn merge_function_maps(dest: &mut HashMap<String, Vec<String>>, mut src: HashMap<String, Vec<String>>) {
    dest.extend(src.drain());
}
