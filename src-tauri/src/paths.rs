use std::path::PathBuf;

pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn new(home: PathBuf) -> Self {
        Self { root: home.join("CogniTrade") }
    }

    pub fn library(&self) -> PathBuf { self.root.join("library") }
    pub fn screenshots(&self) -> PathBuf { self.root.join("screenshots") }
    pub fn data(&self) -> PathBuf { self.root.join("data") }
    pub fn lancedb(&self) -> PathBuf { self.data().join("vec.db") }
    pub fn chat_db(&self) -> PathBuf { self.data().join("chat.db") }
    pub fn logs(&self) -> PathBuf { self.root.join("logs") }

    pub fn ensure_all(&self) -> std::io::Result<()> {
        for p in [&self.library(), &self.screenshots(), &self.data(), &self.logs()] {
            std::fs::create_dir_all(p)?;
        }
        Ok(())
    }

    pub fn from_env() -> Self {
        let home = dirs::home_dir().expect("home dir resolvable");
        Self::new(home)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensures_all_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Paths::new(tmp.path().to_path_buf());
        p.ensure_all().unwrap();
        assert!(p.library().is_dir());
        assert!(p.screenshots().is_dir());
        assert!(p.data().is_dir());
        assert!(p.logs().is_dir());
    }
}
