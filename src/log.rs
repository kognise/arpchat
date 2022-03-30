use directories::ProjectDirs;
use simplelog::*;
use std::{fs, path};

fn make_path() -> Option<path::PathBuf> {
    let dirs = ProjectDirs::from("dev", "kognise", "arpchat")?;
    let dir = dirs.data_dir();
    fs::create_dir_all(&dir).expect(&format!("failed to create log dir {}", dir.display()));

    Some(dir.join("info.log"))
}

pub fn init() {
    if let Some(path) = make_path() {
        CombinedLogger::init(vec![WriteLogger::new(
            LevelFilter::Info,
            Config::default(),
            fs::File::create(&path).expect(&format!("failed to open logfile {}", path.display())),
        )])
        .unwrap();
    }
}
