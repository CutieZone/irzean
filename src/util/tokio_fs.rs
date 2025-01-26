use std::{
    io,
    path::{Path, PathBuf},
};

use grass::Fs;
use tokio::{fs, runtime::Handle, task::block_in_place};

#[derive(Debug)]
pub struct TokioFs;

impl Fs for TokioFs {
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        block_in_place(|| Handle::current().block_on(async { fs::read(path).await }))
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        block_in_place(|| Handle::current().block_on(async { fs::canonicalize(path).await }))
    }
}
