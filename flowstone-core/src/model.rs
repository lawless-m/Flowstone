pub struct Note {
    pub path: String,
    pub title: String,
    pub body: String,
    pub size: u64,
    pub modified: f64,
}

pub struct Link {
    pub source: String,
    pub target: String,
}

pub struct LoadStats {
    pub notes: usize,
    pub links: usize,
}
