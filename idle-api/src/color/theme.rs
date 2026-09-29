#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Synthwave,
    Cyberpunk,
    Neon,
    Aurora,
    Monokai,
    Matrix,
}

impl Theme {
    pub fn palette(&self) -> &[(u8, u8, u8)] {
        match self {
            Self::Synthwave => &[(255, 0, 102), (0, 255, 255), (255, 153, 0)],
            Self::Cyberpunk => &[(255, 255, 0), (0, 255, 255), (255, 0, 255)],
            Self::Neon => &[(0, 255, 0), (255, 0, 255), (0, 255, 255)],
            Self::Aurora => &[(0, 255, 127), (127, 0, 255), (0, 127, 255)],
            Self::Monokai => &[(249, 38, 114), (166, 226, 46), (102, 217, 239)],
            Self::Matrix => &[(0, 255, 0), (0, 200, 0), (0, 150, 0)],
        }
    }
}

impl std::str::FromStr for Theme {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "synthwave" => Ok(Self::Synthwave),
            "cyberpunk" => Ok(Self::Cyberpunk),
            "neon" => Ok(Self::Neon),
            "aurora" => Ok(Self::Aurora),
            "monokai" => Ok(Self::Monokai),
            "matrix" => Ok(Self::Matrix),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for Theme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Synthwave => write!(f, "synthwave"),
            Self::Cyberpunk => write!(f, "cyberpunk"),
            Self::Neon => write!(f, "neon"),
            Self::Aurora => write!(f, "aurora"),
            Self::Monokai => write!(f, "monokai"),
            Self::Matrix => write!(f, "matrix"),
        }
    }
}
