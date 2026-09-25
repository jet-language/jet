#[derive(Clone, Debug)]
pub struct JetTempDirOwner {
    pub(crate) path: String,
    cleanup: std::rc::Rc<()>,
}

#[derive(Clone, Debug)]
pub struct JetTempFileOwner {
    pub(crate) path: String,
    cleanup: std::rc::Rc<()>,
}

#[derive(Clone, Debug)]
pub struct JetFileLockOwner {
    pub(crate) path: String,
    cleanup: std::rc::Rc<()>,
}

fn jet_fs_drop_owned_path(path: &str, cleanup: &std::rc::Rc<()>, is_dir: bool) {
    if std::rc::Rc::strong_count(cleanup) == 1 {
        if is_dir {
            let _ = std::fs::remove_dir_all(path);
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for JetTempDirOwner {
    fn drop(&mut self) {
        jet_fs_drop_owned_path(&self.path, &self.cleanup, true);
    }
}

impl Drop for JetTempFileOwner {
    fn drop(&mut self) {
        jet_fs_drop_owned_path(&self.path, &self.cleanup, false);
    }
}

impl Drop for JetFileLockOwner {
    fn drop(&mut self) {
        jet_fs_drop_owned_path(&self.path, &self.cleanup, false);
    }
}
