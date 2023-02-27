use std::io;
use std::fs::File;
use std::path::Path;

use std::io::prelude::Read;
use std::io::prelude::Write;

pub fn write(text: &str, path: &Path) -> io::Result<()> {
    let mut f = File::create(path)?;
    f.write_all(text.as_bytes())
}

pub fn read(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut s = String::new();
    match f.read_to_string(&mut s) {
        Ok(_) => Ok(s),
        Err(e) => Err(e),
    }
}
