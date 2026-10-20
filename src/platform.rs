use std::path::PathBuf;

/// Finds `program` in the `PATH` the way a shell would, adding `.exe` on
/// Windows.
pub fn find_program(program: &str) -> Option<PathBuf> {
    let file = format!("{program}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&file))
        .find(|path| path.is_file())
}
