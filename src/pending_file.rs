use std::path::PathBuf;

/// Remove somente o arquivo criado por esta operacao se ela falhar ou cancelar.
/// Declare antes do handle de escrita, para fechar o handle antes da limpeza.
#[derive(Default)]
pub struct PendingFile(Option<PathBuf>);

impl PendingFile {
    pub fn track(&mut self, path: PathBuf) { self.0 = Some(path); }
    pub fn commit(&mut self) { self.0 = None; }
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            if let Err(error) = std::fs::remove_file(path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(path = %path.display(), %error, "nao foi possivel limpar upload incompleto");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_uncommitted_owned_files_are_removed() {
        let path = PathBuf::from(format!(".test-upload-{}", rand::random::<u64>()));
        std::fs::write(&path, b"partial").unwrap();
        {
            let mut cleanup = PendingFile::default();
            cleanup.track(path.clone());
        }
        assert!(!path.exists());
        std::fs::write(&path, b"complete").unwrap();
        {
            let mut cleanup = PendingFile::default();
            cleanup.track(path.clone());
            cleanup.commit();
        }
        assert!(path.exists());
        drop(PendingFile::default());
        assert!(path.exists());
        std::fs::remove_file(path).unwrap();
    }
}
