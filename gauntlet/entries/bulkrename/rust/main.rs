use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let directory = PathBuf::from(env::args().nth(1).expect("usage: bulkrename DIR"));
    let mut photos = Vec::new();
    let mut files = 0;
    for entry in fs::read_dir(&directory).expect("cannot list directory") {
        let entry = entry.expect("cannot read directory entry");
        let path = entry.path();
        if path.is_file() {
            files += 1;
            let name = path.file_name().unwrap().to_str().unwrap();
            if let Some(raw) = name.strip_prefix("IMG_").and_then(|s| s.strip_suffix(".jpeg")) {
                if let Ok(number) = raw.parse::<u32>() {
                    photos.push((number, path));
                }
            }
        }
    }
    photos.sort_by_key(|(number, _)| *number);
    for (index, (_, source)) in photos.iter().enumerate() {
        let name = source.file_name().unwrap().to_str().unwrap();
        let target_name = format!("photo-{:04}.jpg", index + 1);
        fs::rename(source, directory.join(&target_name)).expect("cannot rename photo");
        println!("renamed {} -> {}", name, target_name);
    }
    println!("renamed {} skipped {}", photos.len(), files - photos.len());
}
